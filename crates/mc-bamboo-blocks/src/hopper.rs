//! 漏斗，官方 `HopperBlockEntity` 和它用的取放物品算法。
//!
//! 每刻把朝向那一格的容器塞一个物品，再从上面一格的容器吸一个，冷却 8 刻。
//! 堆肥桶没有物品栏，但是官方把它也当成容器，见 [`crate::composter`]。
//! 这个世界没有物品实体，所以官方从地上和上方吸物品实体的那一段没写：方块位置上面没有容器时
//! 什么也不做，和上面被完整方块压住时的官方行为一样。

use std::any::Any;

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::BlockAncestor;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    block_entity::BlockEntity,
    container::{Container, item_max, remove_item, same_item_same_components},
    state::{ENABLED, FACING_HOPPER},
    world::World,
};

/// `HopperBlockEntity.MOVE_ITEM_SPEED`。
pub const MOVE_ITEM_SPEED: i32 = 8;
/// `HopperBlockEntity.HOPPER_CONTAINER_SIZE`。
pub const HOPPER_CONTAINER_SIZE: usize = 5;
/// `HopperBlockEntity.NO_COOLDOWN_TIME`。
pub const NO_COOLDOWN_TIME: i32 = -1;

/// `HopperBlockEntity`。槽位内容在 [`Container`] 里，这里只有冷却和时间。
pub struct HopperEntity {
    pos: BlockPos,
    /// `cooldownTime`。刚放下时是 -1。
    cooldown_time: i32,
    /// `tickedGameTime`：上一次 tick 的 `gameTime`。相邻的漏斗靠它决定要不要少等一刻。
    ticked_game_time: i64,
}

impl HopperEntity {
    /// `newBlockEntity`：刚放下的漏斗。
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            cooldown_time: NO_COOLDOWN_TIME,
            ticked_game_time: 0,
        }
    }

    /// `isOnCooldown`：还在冷却里。
    pub fn is_on_cooldown(&self) -> bool {
        self.cooldown_time > 0
    }

    /// `isOnCustomCooldown`：冷却比正常的一轮长。只在往空漏斗里放东西时用到。
    pub fn is_on_custom_cooldown(&self) -> bool {
        self.cooldown_time > MOVE_ITEM_SPEED
    }

    /// `setCooldown`。
    pub fn set_cooldown(&mut self, cooldown_time: i32) {
        self.cooldown_time = cooldown_time;
    }

    /// 现在的冷却时间，测试和存档用。
    pub fn cooldown_time(&self) -> i32 {
        self.cooldown_time
    }

    /// `tickedGameTime`。
    pub fn ticked_game_time(&self) -> i64 {
        self.ticked_game_time
    }
}

impl BlockEntity for HopperEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        push_items_tick(world, self.pos, self);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// 在 `pos` 的容器上运行一段只读代码。这一格不是容器时是 `None`。
///
/// [`Level::with_container`](crate::Level::with_container) 不返回泛型结果，是为了让
/// `Level` 能当 trait object 用；要结果就借这个函数带出来。
pub(crate) fn query<R>(
    world: &mut World,
    pos: BlockPos,
    f: &mut dyn FnMut(&dyn Container) -> R,
) -> Option<R> {
    let mut result = None;
    let present = world.with_container(pos, &mut |container| {
        result = Some(f(&*container));
    });
    present.then_some(result).flatten()
}

/// `HopperBlockEntity.pushItemsTick`。滴答器每刻调用它。
pub fn push_items_tick(world: &mut World, pos: BlockPos, entity: &mut HopperEntity) {
    entity.cooldown_time -= 1;
    entity.ticked_game_time = world.game_time();
    if !entity.is_on_cooldown() {
        entity.set_cooldown(0);
        try_move_items(world, pos, entity);
    }
}

/// `tryMoveItems`：先往外送，送不出去再看能不能往里吸。
fn try_move_items(world: &mut World, pos: BlockPos, entity: &mut HopperEntity) -> bool {
    if entity.is_on_cooldown() {
        return false;
    }
    let state = world.block_state(pos);
    if !state.get(ENABLED) {
        return false;
    }

    let mut changed = false;
    if !container_is_empty(world, pos) {
        changed = eject_items(world, pos, entity);
    }
    if !inventory_full(world, pos) {
        changed |= suck_in_items(world, pos, entity);
    }
    if changed {
        entity.set_cooldown(MOVE_ITEM_SPEED);
        set_changed(world, pos);
    }
    changed
}

/// 漏斗自己的物品栏是不是空的。
fn container_is_empty(world: &mut World, pos: BlockPos) -> bool {
    query(world, pos, &mut |container| container.is_empty()).unwrap_or(true)
}

/// `inventoryFull`：每一格都有东西，而且数量都到物品自己的堆叠上限。
fn inventory_full(world: &mut World, pos: BlockPos) -> bool {
    query(world, pos, &mut |container| {
        (0..container.size()).all(|slot| {
            let stack = container.get(slot);
            !stack.is_empty() && stack.count == item_max(&stack)
        })
    })
    .unwrap_or(false)
}

/// `ejectItems`：把朝向那一格的容器塞满一个物品。
fn eject_items(world: &mut World, pos: BlockPos, entity: &mut HopperEntity) -> bool {
    let facing = world.block_state(pos).get(FACING_HOPPER);
    let destination = pos.relative(facing);
    let direction = facing.opposite();

    // `getContainerAt`：目标不是容器时什么也不做。世界里的实体不是容器。
    let mut full = None;
    if !world.with_container(destination, &mut |container| {
        full = Some(is_full_container(container, Some(direction)));
    }) {
        return false;
    }
    if full != Some(false) {
        return false;
    }

    for slot in 0..HOPPER_CONTAINER_SIZE {
        let Some(item) = query(world, pos, &mut |container| container.get(slot)) else {
            continue;
        };
        if item.is_empty() {
            continue;
        }
        let mut taken = ItemStack::empty();
        if !world.with_container(pos, &mut |container| {
            taken = remove_item(container, slot, 1);
        }) {
            continue;
        }
        let mut inserted = None;
        world.with_container(destination, &mut |container| {
            inserted = Some(add_item(container, taken.clone(), Some(direction)));
        });
        let Some(inserted) = inserted else {
            // 目标已经不在了，把拿出来的整堆放回去。
            world.with_container(pos, &mut |container| container.set(slot, item.clone()));
            continue;
        };

        if inserted.remaining.is_empty() {
            let destination_state = world.block_state(destination);
            if inserted.was_empty && destination_state.is_a(BlockAncestor::HopperBlock) {
                apply_hopper_cooldown(world, destination, Some(entity.ticked_game_time));
            }
            set_changed(world, destination);
            return true;
        }

        // 放不进去，槽位恢复成原来的整堆。官方靠的是同一个对象，这里是克隆，要显式写回。
        world.with_container(pos, &mut |container| container.set(slot, item.clone()));
    }
    false
}

/// 往空漏斗里放东西时，官方让目标漏斗少等一刻。
///
/// `from_ticked_game_time` 是来源的位置上也有漏斗时的 `tickedGameTime`。来源不是漏斗
/// （比如投掷器）时是 `None`，目标漏斗不会少等。
pub(crate) fn apply_hopper_cooldown(
    world: &mut World,
    pos: BlockPos,
    from_ticked_game_time: Option<i64>,
) {
    world.with_hopper(pos, &mut |destination| {
        if destination.is_on_custom_cooldown() {
            return;
        }
        let skip = i32::from(
            from_ticked_game_time.is_some_and(|from| destination.ticked_game_time >= from),
        );
        destination.set_cooldown(MOVE_ITEM_SPEED - skip);
    });
}

/// `suckInItems`：从上面一格的容器里拿一个物品。
///
/// 官方在上方没有容器时会去吸落在漏斗里和上方的物品实体，这个世界没有物品实体，那一段不写。
fn suck_in_items(world: &mut World, pos: BlockPos, entity: &mut HopperEntity) -> bool {
    let above = pos.above();
    let mut slots = None;
    if !world.with_container(above, &mut |container| {
        slots = Some(container.slots_for_face(Direction::Down).map_or_else(
            || (0..container.size()).collect::<Vec<_>>(),
            <[usize]>::to_vec,
        ));
    }) {
        return false;
    }
    let Some(slots) = slots else {
        return false;
    };
    // 上面的容器也是漏斗时，`from instanceof HopperBlockEntity` 成立。
    let mut from_ticked_game_time = None;
    if world.block_state(above).is_a(BlockAncestor::HopperBlock) {
        world.with_hopper(above, &mut |hopper| {
            from_ticked_game_time = Some(hopper.ticked_game_time());
        });
    }

    for slot in slots {
        let Some(item) = query(world, above, &mut |container| container.get(slot)) else {
            continue;
        };
        if item.is_empty() {
            continue;
        }
        let can_take = query(world, above, &mut |container| {
            can_take_item_from_container(container, &item, slot, Direction::Down)
        })
        .unwrap_or(false);
        if !can_take {
            continue;
        }

        let mut taken = ItemStack::empty();
        if !world.with_container(above, &mut |container| {
            taken = remove_item(container, slot, 1);
        }) {
            continue;
        }
        let mut inserted = None;
        world.with_container(pos, &mut |container| {
            inserted = Some(add_item(container, taken.clone(), None));
        });
        let Some(inserted) = inserted else {
            world.with_container(above, &mut |container| container.set(slot, item.clone()));
            continue;
        };

        if inserted.remaining.is_empty() {
            if inserted.was_empty {
                let skip =
                    from_ticked_game_time.is_some_and(|from| entity.ticked_game_time >= from);
                entity.set_cooldown(MOVE_ITEM_SPEED - i32::from(skip));
            }
            set_changed(world, above);
            return true;
        }

        world.with_container(above, &mut |container| container.set(slot, item.clone()));
    }
    false
}

/// `getSlots` 加 `isFullContainer`：这些槽都有东西并且都到了堆叠上限。
fn is_full_container(container: &dyn Container, direction: Option<Direction>) -> bool {
    let slots = slots_for_face(container, direction);
    slots.iter().all(|&slot| {
        let stack = container.get(slot);
        stack.count >= item_max(&stack)
    })
}

/// `getSlots`：`WorldlyContainer` 按面取，否则整张表。
fn slots_for_face(container: &dyn Container, direction: Option<Direction>) -> Vec<usize> {
    if container.is_worldly()
        && let Some(direction) = direction
    {
        container
            .slots_for_face(direction)
            .map_or_else(Vec::new, <[usize]>::to_vec)
    } else {
        (0..container.size()).collect()
    }
}

/// `canPlaceItemInContainer`。
fn can_place_item_in_container(
    container: &dyn Container,
    stack: &ItemStack,
    slot: usize,
    direction: Option<Direction>,
) -> bool {
    container.can_place_item(slot, stack)
        && !(container.is_worldly()
            && !container.can_place_item_through_face(slot, stack, direction))
}

/// `canTakeItemFromContainer`。
///
/// 官方还把放进来的容器（`into`）传给 `canTakeItem`，现有的容器都没有用它，所以这里省掉。
fn can_take_item_from_container(
    container: &dyn Container,
    stack: &ItemStack,
    slot: usize,
    direction: Direction,
) -> bool {
    container.can_take_item(slot, stack)
        && !(container.is_worldly()
            && !container.can_take_item_through_face(slot, stack, direction))
}

/// `canMergeItems`。
fn can_merge_items(current: &ItemStack, moving: &ItemStack) -> bool {
    current.count <= item_max(current) && same_item_same_components(current, moving)
}

/// `addItem` 的结果。
pub struct Added {
    /// 没放进去、还拿在手上的那一堆。空表示全部放进去了。
    pub remaining: ItemStack,
    /// 目标容器原来整个是空的。往空漏斗里放东西时决定冷却。
    pub was_empty: bool,
}

/// 官方 `HopperBlockEntity.addItem(from, container, stack, direction)`。
///
/// 冷却要用到的来源信息在调用方手里，这里只负责把物品放进目标容器。
pub fn add_item(
    container: &mut dyn Container,
    stack: ItemStack,
    direction: Option<Direction>,
) -> Added {
    let slots = slots_for_face(container, direction);
    let mut remaining = stack;
    let mut was_empty = false;
    for slot in slots {
        if remaining.is_empty() {
            break;
        }
        let inserted = try_move_in_item(container, remaining, slot, direction);
        was_empty |= inserted.was_empty && inserted.success;
        remaining = inserted.remaining;
    }
    Added {
        remaining,
        was_empty,
    }
}

/// `tryMoveInItem` 的结果。
struct Moved {
    remaining: ItemStack,
    success: bool,
    was_empty: bool,
}

/// 官方 `HopperBlockEntity.tryMoveInItem`：往一格放，返回没放进去的部分。
fn try_move_in_item(
    container: &mut dyn Container,
    mut stack: ItemStack,
    slot: usize,
    direction: Option<Direction>,
) -> Moved {
    let current = container.get(slot);
    let mut success = false;
    let mut was_empty = false;
    if can_place_item_in_container(container, &stack, slot, direction) {
        was_empty = container.is_empty();
        if current.is_empty() {
            container.set(slot, stack.clone());
            stack = ItemStack::empty();
            success = true;
        } else if can_merge_items(&current, &stack) {
            let space = item_max(&stack) - current.count;
            let count = stack.count.min(space);
            stack.count -= count;
            if stack.count <= 0 {
                stack = ItemStack::empty();
            }
            let mut merged = current;
            merged.count += count;
            container.set(slot, merged);
            success = count > 0;
        }
    }
    Moved {
        remaining: stack,
        success,
        was_empty,
    }
}

/// `BlockEntity.setChanged`：内容变了，记下变化并告诉旁边和隔一格的比较器。
///
/// 双箱的 `CompoundContainer.setChanged` 两半都通知，所以位置由 [`Level::container_positions`]
/// 按合并后的容器给出；每一半用自己那一格的状态，和官方一样。
pub fn set_changed(world: &mut World, pos: BlockPos) {
    for changed in world.container_positions(pos) {
        let state = world.block_state(changed);
        if state.is_air() {
            continue;
        }
        // 堆肥桶的容器不是方块实体：官方 `InputContainer` / `OutputContainer` 的 `setChanged`
        // 只加层或清空（在 [`crate::composter`] 里已经做过），再调一次时槽已空或状态没变，
        // 不会通知比较器。等级变化引起的通知走的是 `setBlock`。
        // A composter's container is not a block entity: vanilla `InputContainer` /
        // `OutputContainer.setChanged` only adds a layer or empties it (already done in
        // [`crate::composter`]); a second call finds the slot empty or the state unchanged and
        // notifies no comparator. Level changes reach comparators through `setBlock`.
        if state.is_a(BlockAncestor::ComposterBlock) {
            continue;
        }
        world.update_neighbour_for_output_signal(changed, state.block());
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{BlockState, BlockTicks, state::ENABLED, testing::TestLevel};

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn empty(count: usize) -> Vec<ItemStack> {
        vec![ItemStack::empty(); count]
    }

    fn hopper_with_ten_dirt() -> Vec<ItemStack> {
        let mut slots = empty(5);
        slots[0] = ItemStack::new(items::DIRT, 10);
        slots
    }

    /// 放好一格漏斗和它的方块实体。
    fn placed_hopper<'a>(
        level: &'a mut TestLevel,
        ticks: &'a mut BlockTicks,
        pos: BlockPos,
    ) -> World<'a> {
        level.set(pos, BlockState(blocks::HOPPER));
        let mut world = World::new(level, ticks);
        world.set_block_entity(Box::new(HopperEntity::new(pos)));
        world
    }

    fn slot(world: &mut World, pos: BlockPos, index: usize) -> ItemStack {
        query(world, pos, &mut |container| container.get(index)).expect("这一格应该是容器")
    }

    fn cooldown(world: &World, pos: BlockPos) -> i32 {
        world
            .block_entity::<HopperEntity>(pos)
            .expect("漏斗实体应该还在")
            .cooldown_time()
    }

    #[test]
    fn a_hopper_ejects_one_item_then_waits_eight_ticks() {
        let below = POS.below();
        let mut level = TestLevel::new();
        level.set(below, BlockState(blocks::CHEST));
        level.set_container(POS, hopper_with_ten_dirt());
        level.set_container(below, empty(27));
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, POS);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::DIRT, 1));
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 9));
        assert_eq!(cooldown(&world, POS), MOVE_ITEM_SPEED);

        // 冷却 8 刻：第 2 到第 8 刻都不动，第 9 刻再送一个。
        for _ in 0..7 {
            world.tick_block_entities();
        }
        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::DIRT, 1));
        world.tick_block_entities();
        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::DIRT, 2));
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 8));
    }

    #[test]
    fn a_locked_hopper_does_nothing() {
        let below = POS.below();
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::HOPPER).with(ENABLED, false));
        level.set(below, BlockState(blocks::CHEST));
        level.set_container(POS, hopper_with_ten_dirt());
        level.set_container(below, empty(27));
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.set_block_entity(Box::new(HopperEntity::new(POS)));

        world.tick_block_entities();
        assert!(slot(&mut world, below, 0).is_empty());
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 10));
    }

    #[test]
    fn a_hopper_sucks_one_item_from_the_container_above() {
        let above = POS.above();
        let mut level = TestLevel::new();
        level.set(above, BlockState(blocks::CHEST));
        level.set_container(POS, empty(5));
        let mut chest = empty(27);
        chest[3] = ItemStack::new(items::DIRT, 5);
        level.set_container(above, chest);
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, POS);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 1));
        assert_eq!(slot(&mut world, above, 3), ItemStack::new(items::DIRT, 4));
    }

    #[test]
    fn a_full_hopper_does_not_suck() {
        let above = POS.above();
        let mut level = TestLevel::new();
        level.set(above, BlockState(blocks::CHEST));
        level.set_container(POS, vec![ItemStack::new(items::STONE, 64); 5]);
        let mut chest = empty(27);
        chest[0] = ItemStack::new(items::DIRT, 5);
        level.set_container(above, chest);
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, POS);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, above, 0), ItemStack::new(items::DIRT, 5));
    }

    #[test]
    fn the_comparator_reads_the_hopper_contents() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::HOPPER));
        let mut slots = empty(5);
        slots[0] = ItemStack::new(items::STONE, 64);
        level.set_container(POS, slots);
        let mut ticks = BlockTicks::new();
        let world = World::new(&mut level, &mut ticks);

        // 5 格里的 1 摞满石头：1/5 * 14 = 2.8，向下取整再加一。
        assert_eq!(
            world
                .block_state(POS)
                .analog_output_signal(&world, POS, Direction::Up),
            3
        );
    }
}
