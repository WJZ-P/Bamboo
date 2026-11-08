//! 投掷器，官方 `DropperBlock.dispenseFrom`。
//!
//! 方块被信号触发后隔 4 刻排一个计划刻，到期时从 9 个槽里随机挑一个物品：
//! 朝着的方向有容器就塞一个进去，没有就把物品抛出去。这个世界没有物品实体，
//! 抛出去的那一个直接消失，但官方抛出去时要取的随机数照取，见 [`spawn_item_random`]。
//!
//! 发射器走 [`crate::dispense`]，不走这个函数。空槽的 `levelEvent(1001)` 两边相同。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::BlockAncestor;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    container::{copy_with_count, stack_limit},
    hopper::{add_item, apply_hopper_cooldown, query, set_changed},
    state::{BlockState, FACING},
    world::World,
};

/// `DispenserBlockEntity.getRandomSlot`。
///
/// 每个非空的槽都抽一次 `nextInt(replaceOdds++)`，抽到 0 就把结果换成这一格，
/// 越靠后的槽越容易被选中。`occupied` 是 9 个槽是不是非空。
pub(crate) fn random_slot(world: &World, occupied: &[bool]) -> i32 {
    let mut replace_slot = -1;
    let mut replace_odds = 1;
    for (slot, &non_empty) in occupied.iter().enumerate() {
        if !non_empty {
            continue;
        }
        let roll = world.random().next_int(replace_odds);
        replace_odds += 1;
        if roll == 0 {
            replace_slot = slot as i32;
        }
    }
    replace_slot
}

/// `DefaultDispenseItemBehavior.spawnItem` 取的那几个随机数。
///
/// 一个 `nextDouble` 算 `pow`，再加三个 `triangle`，每个 `triangle` 取两个 `nextDouble`。
/// 物品实体不建，随机数不能少取。
pub(crate) fn spawn_item_random(world: &mut World) {
    let _pow = world.random().next_double() * 0.1 + 0.2;
    for _ in 0..3 {
        let _ = world.random().triangle(0.2, 0.017_227_5 * 6.0);
    }
}

/// 从 `stack` 里拿走一个。官方 `split(1)` 之后剩下的那一堆。
pub(crate) fn without_one(stack: &ItemStack) -> ItemStack {
    if stack.count <= 1 {
        ItemStack::empty()
    } else {
        copy_with_count(stack, stack.count - 1)
    }
}

/// `BaseContainerBlockEntity.setItem` 里的 `limitSize(getMaxStackSize(itemStack))`。
pub(crate) fn limited(stack: ItemStack) -> ItemStack {
    let mut stack = stack;
    if !stack.is_empty() {
        stack.count = stack.count.min(stack_limit(&stack));
    }
    stack
}

/// `DispenserBlock.dispenseFrom` 在槽位是 `-1` 时的那一下。
///
/// 计划刻走 [`crate::dispense::dispense_from`]，空槽的 1001 已经包含在里面。
/// 这个函数留着 main 上的那一截：有物品时不发射，空槽发 1001。
///
/// The `-1` slot half of `DispenserBlock.dispenseFrom`.
///
/// The scheduled tick uses [`crate::dispense::dispense_from`], which already plays 1001
/// for an empty slot. This is the function main added: items present do nothing, an empty
/// slot plays 1001.
pub fn click_if_empty(world: &mut World, pos: BlockPos) {
    let Some(occupied) = occupied_slots(world, pos) else {
        return;
    };
    if random_slot(world, &occupied) < 0 {
        world.level_event(1001, pos, 0);
    }
}

/// 九个槽是不是非空。没有方块实体时是 `None`。
pub(crate) fn occupied_slots(world: &mut World, pos: BlockPos) -> Option<Vec<bool>> {
    let mut occupied = None;
    if !world.with_container(pos, &mut |container| {
        occupied = Some(
            (0..container.size())
                .map(|slot| !container.get(slot).is_empty())
                .collect::<Vec<_>>(),
        );
    }) {
        return None;
    }
    occupied
}

/// `DropperBlock.dispenseFrom`。
pub fn dispense_from(world: &mut World, pos: BlockPos, state: BlockState) {
    // 官方找不到方块实体时只打一条警告。
    let Some(occupied) = occupied_slots(world, pos) else {
        return;
    };

    let slot = random_slot(world, &occupied);
    if slot < 0 {
        // `levelEvent(1001)`：空的投掷器，客户端自己放音效。
        world.level_event(1001, pos, 0);
        return;
    }

    let slot = slot as usize;
    let Some(item) = query(world, pos, &mut |container| container.get(slot)) else {
        return;
    };
    if item.is_empty() {
        return;
    }

    let direction = state.get(FACING);
    let destination = pos.relative(direction);
    // 官方的 `getContainerAt` 先看方块里的容器，再看这个世界里有没有实体是容器。
    let mut inserted = None;
    world.with_container(destination, &mut |container| {
        inserted = Some(add_item(
            container,
            copy_with_count(&item, 1),
            Some(direction.opposite()),
        ));
    });

    let remaining = if let Some(inserted) = inserted {
        if inserted.remaining.is_empty() {
            let destination_state = world.block_state(destination);
            if inserted.was_empty && destination_state.is_a(BlockAncestor::HopperBlock) {
                // 来源是投掷器，不是漏斗，所以目标漏斗不会少等一刻。
                apply_hopper_cooldown(world, destination, None);
            }
            // `tryMoveInItem` 每塞进一格都会 `setChanged`，这里统一通知一次。
            set_changed(world, destination);
            without_one(&item)
        } else {
            // 一个都放不进去，物品原样留在投掷器里。
            item.clone()
        }
    } else {
        // 官方在这里生成物品实体。没有实体，东西凭空消失，随机数照取。
        // `DefaultDispenseItemBehavior.dispense`：先 `execute`（随机数），再 1000，再 2000。
        spawn_item_random(world);
        world.level_event(1000, pos, 0);
        world.level_event(2000, pos, direction.id());
        without_one(&item)
    };

    // `setItem` 会 `setChanged`，比较器跟着变。
    let remaining = limited(remaining);
    world.with_container(pos, &mut |container| container.set(slot, remaining.clone()));
    set_changed(world, pos);
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Direction;
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{BlockTicks, LegacyRandom, testing::TestLevel};

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn empty(count: usize) -> Vec<ItemStack> {
        vec![ItemStack::empty(); count]
    }

    fn slot(world: &mut World, pos: BlockPos, index: usize) -> ItemStack {
        query(world, pos, &mut |container| container.get(index)).expect("这一格应该是容器")
    }

    fn placed_dropper<'a>(
        level: &'a mut TestLevel,
        ticks: &'a mut BlockTicks,
        facing: Direction,
    ) -> World<'a> {
        let mut world = World::new(level, ticks);
        world.set_block(
            POS,
            BlockState(blocks::DROPPER).with(FACING, facing),
            crate::world::UPDATE_ALL,
        );
        world
    }

    #[test]
    fn a_dropper_moves_one_item_into_the_container_it_faces() {
        let below = POS.below();
        let mut level = TestLevel::new();
        level.set(below, BlockState(blocks::CHEST));
        level.set_container(below, empty(27));
        let mut dropper = empty(9);
        dropper[4] = ItemStack::new(items::STONE, 3);
        level.set_container(POS, dropper);
        let mut ticks = BlockTicks::new();
        let mut world = placed_dropper(&mut level, &mut ticks, Direction::Down);

        let state = world.block_state(POS);
        dispense_from(&mut world, POS, state);

        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::STONE, 1));
        assert_eq!(slot(&mut world, POS, 4), ItemStack::new(items::STONE, 2));

        // 只抽了一次 `nextInt(1)`，没有生成物品实体。
        let twin = LegacyRandom::new(0);
        twin.next_int(1);
        assert_eq!(world.random().seed(), twin.seed());
    }

    #[test]
    fn a_dropper_into_a_full_container_keeps_the_item() {
        let below = POS.below();
        let mut level = TestLevel::new();
        level.set(below, BlockState(blocks::CHEST));
        level.set_container(below, vec![ItemStack::new(items::STONE, 64); 27]);
        let mut dropper = empty(9);
        dropper[4] = ItemStack::new(items::STONE, 3);
        level.set_container(POS, dropper);
        let mut ticks = BlockTicks::new();
        let mut world = placed_dropper(&mut level, &mut ticks, Direction::Down);

        let state = world.block_state(POS);
        dispense_from(&mut world, POS, state);

        assert_eq!(slot(&mut world, POS, 4), ItemStack::new(items::STONE, 3));
    }

    #[test]
    fn a_dropper_without_a_container_loses_the_item_but_draws_the_same_numbers() {
        let mut level = TestLevel::new();
        let mut dropper = empty(9);
        dropper[4] = ItemStack::new(items::STONE, 3);
        level.set_container(POS, dropper);
        let mut ticks = BlockTicks::new();
        let mut world = placed_dropper(&mut level, &mut ticks, Direction::Up);

        let state = world.block_state(POS);
        dispense_from(&mut world, POS, state);

        assert_eq!(slot(&mut world, POS, 4), ItemStack::new(items::STONE, 2));

        // `getRandomSlot` 一次 `nextInt(1)`，`spawnItem` 一个 double 加三个 triangle。
        let twin = LegacyRandom::new(0);
        twin.next_int(1);
        twin.next_double();
        for _ in 0..3 {
            twin.triangle(0.2, 0.017_227_5 * 6.0);
        }
        assert_eq!(world.random().seed(), twin.seed());
    }

    #[test]
    fn the_random_slot_only_considers_non_empty_slots() {
        let below = POS.below();
        let mut level = TestLevel::new();
        level.set(below, BlockState(blocks::CHEST));
        level.set_container(below, empty(27));
        let mut dropper = empty(9);
        dropper[0] = ItemStack::new(items::STONE, 1);
        dropper[8] = ItemStack::new(items::DIRT, 1);
        level.set_container(POS, dropper);
        let mut ticks = BlockTicks::new();
        let mut world = placed_dropper(&mut level, &mut ticks, Direction::Down);

        let state = world.block_state(POS);
        dispense_from(&mut world, POS, state);

        let twin = LegacyRandom::new(0);
        twin.next_int(1);
        let second = twin.next_int(2);
        let expected = if second == 0 {
            ItemStack::new(items::DIRT, 1)
        } else {
            ItemStack::new(items::STONE, 1)
        };
        assert_eq!(slot(&mut world, below, 0), expected);
        assert_eq!(world.random().seed(), twin.seed());
    }
}
