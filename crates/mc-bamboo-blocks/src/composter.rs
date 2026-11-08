//! 堆肥桶给漏斗和投掷器用的容器，官方 `ComposterBlock.getContainer`。
//!
//! 堆肥桶没有方块实体，官方每次取容器都新建一个。等级不到 7 是 [`Input`]（只从上面收一个能堆肥的
//! 物品），等级 8 是 [`Output`]（只从下面给一个骨粉），等级 7 是 [`Empty`]（哪个面都不收不给）。
//! 放进输入容器会走 `setChanged` → `addLayer`，取走输出容器里的骨粉会走 `empty`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{CompostLayers, items};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    behaviour::{
        common::item_of,
        misc::{COMPOST_MAX_LEVEL, COMPOST_READY, compost_empty, compost_fill},
    },
    container::{Container, item_max},
    state::LEVEL_COMPOSTER,
    world::World,
};

/// 上面那一格，输入容器只对这个面开放。
const TOP_SLOT: &[usize] = &[0];
/// 下面那一格，输出容器只对这个面开放。
const BOTTOM_SLOT: &[usize] = &[0];
/// 这个面没有槽。
const NO_SLOTS: &[usize] = &[];

/// 一次 `setChanged` 要在回调结束之后写进世界的事。
enum Pending {
    /// `InputContainer.setChanged`：按物品的堆肥规则加层，并发 `levelEvent(1500)`。
    Fill(CompostLayers),
    /// `OutputContainer.setChanged`：`empty`，等级归零。
    Empty,
}

/// 这一格是堆肥桶时，给调用方一个和官方 `getContainer` 一样的容器。
///
/// 容器只活在这次调用里。`setItem` / `removeItem` 触发的 `setChanged` 先记下来，
/// 回调返回之后立刻写进世界，漏斗后面的吸和冷却看到的已经是改完的方块。
pub(crate) fn with_container(
    world: &mut World,
    pos: BlockPos,
    f: &mut dyn FnMut(&mut dyn Container),
) -> bool {
    let state = world.block_state(pos);
    if !state.is_a(mc_bamboo_data::BlockAncestor::ComposterBlock) {
        return false;
    }
    let mut pending = Vec::new();
    let level = state.get(LEVEL_COMPOSTER);
    if level == COMPOST_READY {
        let mut container = Output::new(&mut pending);
        f(&mut container);
    } else if level < COMPOST_MAX_LEVEL {
        let mut container = Input::new(&mut pending);
        f(&mut container);
    } else {
        let mut container = Empty;
        f(&mut container);
    }
    for action in pending {
        match action {
            Pending::Fill(layers) => compost_fill(state, world, pos, layers),
            Pending::Empty => compost_empty(state, world, pos),
        }
    }
    true
}

/// 物品注册表上的 `compostable`。堆肥桶认的是这个组件，和右键同一条规则。
fn compostable(stack: &ItemStack) -> Option<CompostLayers> {
    item_of(stack.item).and_then(|item| item.compostable)
}

/// `SimpleContainer.setItem` 里的 `limitSize(getMaxStackSize(stack))`。这两种容器上限都是 1。
fn limited(stack: ItemStack) -> ItemStack {
    if stack.is_empty() {
        return stack;
    }
    let max = 1.min(item_max(&stack));
    if stack.count <= max {
        stack
    } else {
        let mut stack = stack;
        stack.count = max;
        stack
    }
}

/// `ComposterBlock.InputContainer`。
struct Input<'a> {
    slot: ItemStack,
    /// `changed`：这一轮已经堆过肥，上面就不再收。
    changed: bool,
    pending: &'a mut Vec<Pending>,
}

impl<'a> Input<'a> {
    fn new(pending: &'a mut Vec<Pending>) -> Self {
        Self {
            slot: ItemStack::empty(),
            changed: false,
            pending,
        }
    }

    /// `InputContainer.setChanged`。槽里有能堆肥的物品才加层，然后 `removeItemNoUpdate` 把槽清空。
    fn on_changed(&mut self) {
        let Some(layers) = compostable(&self.slot) else {
            return;
        };
        if self.slot.is_empty() {
            return;
        }
        self.changed = true;
        self.pending.push(Pending::Fill(layers));
        self.slot = ItemStack::empty();
    }
}

impl Container for Input<'_> {
    fn size(&self) -> usize {
        1
    }

    fn get(&self, slot: usize) -> ItemStack {
        if slot == 0 {
            self.slot.clone()
        } else {
            ItemStack::empty()
        }
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        if slot != 0 {
            return;
        }
        self.slot = limited(stack);
        self.on_changed();
    }

    fn is_worldly(&self) -> bool {
        true
    }

    fn max_stack_size(&self) -> i32 {
        1
    }

    /// 只有上面那一格。
    fn slots_for_face(&self, direction: Direction) -> Option<&[usize]> {
        if direction == Direction::Up {
            Some(TOP_SLOT)
        } else {
            Some(NO_SLOTS)
        }
    }

    /// `canPlaceItemThroughFace`：还没堆过，而且是从上面放进来的能堆肥物品。
    fn can_place_item_through_face(
        &self,
        _slot: usize,
        stack: &ItemStack,
        direction: Option<Direction>,
    ) -> bool {
        !self.changed && direction == Some(Direction::Up) && compostable(stack).is_some()
    }

    fn can_take_item_through_face(
        &self,
        _slot: usize,
        _stack: &ItemStack,
        _direction: Direction,
    ) -> bool {
        false
    }
}

/// `ComposterBlock.OutputContainer`。槽里一开始就是一个骨粉。
struct Output<'a> {
    slot: ItemStack,
    changed: bool,
    pending: &'a mut Vec<Pending>,
}

impl<'a> Output<'a> {
    fn new(pending: &'a mut Vec<Pending>) -> Self {
        Self {
            slot: ItemStack::new(items::BONE_MEAL, 1),
            changed: false,
            pending,
        }
    }

    /// `OutputContainer.setChanged`：不管槽里现在是什么，都把堆肥桶清空。
    fn on_changed(&mut self) {
        self.pending.push(Pending::Empty);
        self.changed = true;
    }
}

impl Container for Output<'_> {
    fn size(&self) -> usize {
        1
    }

    fn get(&self, slot: usize) -> ItemStack {
        if slot == 0 {
            self.slot.clone()
        } else {
            ItemStack::empty()
        }
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        if slot != 0 {
            return;
        }
        self.slot = limited(stack);
        self.on_changed();
    }

    fn is_worldly(&self) -> bool {
        true
    }

    fn max_stack_size(&self) -> i32 {
        1
    }

    /// 只有下面那一格。
    fn slots_for_face(&self, direction: Direction) -> Option<&[usize]> {
        if direction == Direction::Down {
            Some(BOTTOM_SLOT)
        } else {
            Some(NO_SLOTS)
        }
    }

    fn can_place_item_through_face(
        &self,
        _slot: usize,
        _stack: &ItemStack,
        _direction: Option<Direction>,
    ) -> bool {
        false
    }

    /// `canTakeItemThroughFace`：还没被取走过，而且是从下面拿走的骨粉。
    fn can_take_item_through_face(
        &self,
        _slot: usize,
        stack: &ItemStack,
        direction: Direction,
    ) -> bool {
        !self.changed && direction == Direction::Down && stack.item == items::BONE_MEAL
    }
}

/// `ComposterBlock.EmptyContainer`。等级正好是 7：满了，骨粉还没好。
struct Empty;

impl Container for Empty {
    fn size(&self) -> usize {
        0
    }

    fn get(&self, _slot: usize) -> ItemStack {
        ItemStack::empty()
    }

    fn set(&mut self, _slot: usize, _stack: ItemStack) {}

    fn is_worldly(&self) -> bool {
        true
    }

    fn slots_for_face(&self, _direction: Direction) -> Option<&[usize]> {
        Some(NO_SLOTS)
    }

    fn can_place_item_through_face(
        &self,
        _slot: usize,
        _stack: &ItemStack,
        _direction: Option<Direction>,
    ) -> bool {
        false
    }

    fn can_take_item_through_face(
        &self,
        _slot: usize,
        _stack: &ItemStack,
        _direction: Direction,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockState, BlockTicks, LegacyRandom,
        dispenser::dispense_from,
        hopper::{HopperEntity, MOVE_ITEM_SPEED, query},
        level::Effect,
        state::{FACING, FACING_HOPPER},
        testing::TestLevel,
    };

    const HOPPER: BlockPos = BlockPos::new(0, 64, 0);

    fn empty_slots(count: usize) -> Vec<ItemStack> {
        vec![ItemStack::empty(); count]
    }

    fn composter(level: i32) -> BlockState {
        BlockState(blocks::COMPOSTER).with(LEVEL_COMPOSTER, level)
    }

    fn placed_hopper<'a>(
        level: &'a mut TestLevel,
        ticks: &'a mut BlockTicks,
        pos: BlockPos,
        facing: Direction,
    ) -> World<'a> {
        level.set(pos, BlockState(blocks::HOPPER).with(FACING_HOPPER, facing));
        let mut world = World::new(level, ticks);
        world.set_block_entity(Box::new(HopperEntity::new(pos)));
        world
    }

    fn slot(world: &mut World, pos: BlockPos, index: usize) -> ItemStack {
        query(world, pos, &mut |container| container.get(index)).expect("这一格应该是容器")
    }

    /// 面包是 85%：`nextInt(100)` 落在第一项才加一层。
    fn bread_adds_a_layer(seed: i64) -> bool {
        LegacyRandom::new(seed).next_int(100) < 85
    }

    #[test]
    fn a_hopper_above_composts_one_item_and_may_raise_the_level() {
        let composter_pos = HOPPER.below();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(5));
        let mut slots = empty_slots(5);
        slots[0] = ItemStack::new(items::BREAD, 4);
        level.set_container(HOPPER, slots);
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::Down);

        let adds = bread_adds_a_layer(0);
        world.tick_block_entities();

        assert_eq!(slot(&mut world, HOPPER, 0), ItemStack::new(items::BREAD, 3));
        assert_eq!(
            world.block_state(composter_pos).get(LEVEL_COMPOSTER),
            5 + i32::from(adds)
        );
        // 比较器看的是方块状态里的等级，不看容器槽。
        assert_eq!(world.container_signal(composter_pos), 0);
        assert_eq!(
            world.block_state(composter_pos).analog_output_signal(
                &world,
                composter_pos,
                Direction::Up
            ),
            5 + i32::from(adds)
        );
        assert_eq!(
            world
                .block_entity::<HopperEntity>(HOPPER)
                .expect("漏斗还在")
                .cooldown_time(),
            MOVE_ITEM_SPEED
        );
        let effects = world.into_effects();
        assert_eq!(
            effects,
            [Effect::ComposterFill {
                pos: composter_pos,
                success: adds,
            }]
        );
        // 面包只抽一次 `nextInt(100)`。
        let twin = LegacyRandom::new(0);
        twin.next_int(100);
        assert_eq!(level.random.source().seed(), twin.seed());
    }

    #[test]
    fn a_failed_compost_roll_still_consumes_the_item() {
        let composter_pos = HOPPER.below();
        let seed = (0..200)
            .find(|&seed| !bread_adds_a_layer(seed))
            .expect("200 个种子里总有一次抽不中");
        let mut level = TestLevel::new();
        level.random.source().set_seed(seed);
        level.set(composter_pos, composter(5));
        let mut slots = empty_slots(5);
        slots[0] = ItemStack::new(items::BREAD, 2);
        level.set_container(HOPPER, slots);
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::Down);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, HOPPER, 0), ItemStack::new(items::BREAD, 1));
        assert_eq!(world.block_state(composter_pos).get(LEVEL_COMPOSTER), 5);
        assert_eq!(
            world.into_effects(),
            [Effect::ComposterFill {
                pos: composter_pos,
                success: false,
            }]
        );
    }

    #[test]
    fn a_hopper_skips_items_that_cannot_be_composted() {
        let composter_pos = HOPPER.below();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(2));
        let mut slots = empty_slots(5);
        slots[0] = ItemStack::new(items::STONE, 3);
        slots[1] = ItemStack::new(items::CAKE, 1);
        level.set_container(HOPPER, slots);
        let before = level.random.source().seed();
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::Down);

        world.tick_block_entities();
        // 石头放不进去，蛋糕固定加一层，不取随机数。
        assert_eq!(slot(&mut world, HOPPER, 0), ItemStack::new(items::STONE, 3));
        assert!(slot(&mut world, HOPPER, 1).is_empty());
        assert_eq!(world.block_state(composter_pos).get(LEVEL_COMPOSTER), 3);
        assert_eq!(world.random().seed(), before);
    }

    #[test]
    fn a_full_composter_accepts_nothing() {
        let composter_pos = HOPPER.below();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(COMPOST_MAX_LEVEL));
        let mut slots = empty_slots(5);
        slots[0] = ItemStack::new(items::CAKE, 1);
        level.set_container(HOPPER, slots);
        let before = level.random.source().seed();
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::Down);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, HOPPER, 0), ItemStack::new(items::CAKE, 1));
        assert_eq!(
            world.block_state(composter_pos).get(LEVEL_COMPOSTER),
            COMPOST_MAX_LEVEL
        );
        assert!(world.into_effects().is_empty());
        assert_eq!(level.random.source().seed(), before);
    }

    #[test]
    fn a_side_hopper_cannot_push_into_a_composter() {
        let composter_pos = HOPPER.relative(Direction::North);
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(3));
        let mut slots = empty_slots(5);
        slots[0] = ItemStack::new(items::CAKE, 1);
        level.set_container(HOPPER, slots);
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::North);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, HOPPER, 0), ItemStack::new(items::CAKE, 1));
        assert_eq!(world.block_state(composter_pos).get(LEVEL_COMPOSTER), 3);
    }

    #[test]
    fn a_hopper_below_pulls_bone_meal_and_empties_the_composter() {
        let composter_pos = HOPPER.above();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(COMPOST_READY));
        level.set_container(HOPPER, empty_slots(5));
        let before = level.random.source().seed();
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::Down);

        world.tick_block_entities();
        assert_eq!(
            slot(&mut world, HOPPER, 0),
            ItemStack::new(items::BONE_MEAL, 1)
        );
        assert_eq!(world.block_state(composter_pos).get(LEVEL_COMPOSTER), 0);
        // `empty` 不取随机数，也不发堆肥的 level event。
        assert!(world.into_effects().is_empty());
        assert_eq!(level.random.source().seed(), before);
    }

    #[test]
    fn a_hopper_below_does_not_pull_from_an_input_composter() {
        let composter_pos = HOPPER.above();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(5));
        level.set_container(HOPPER, empty_slots(5));
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::Down);

        world.tick_block_entities();
        assert!(slot(&mut world, HOPPER, 0).is_empty());
        assert_eq!(world.block_state(composter_pos).get(LEVEL_COMPOSTER), 5);
    }

    #[test]
    fn a_full_hopper_leaves_the_bone_meal_in_place() {
        let composter_pos = HOPPER.above();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(COMPOST_READY));
        level.set_container(HOPPER, vec![ItemStack::new(items::DIRT, 64); 5]);
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::Down);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, HOPPER, 0), ItemStack::new(items::DIRT, 64));
        assert_eq!(
            world.block_state(composter_pos).get(LEVEL_COMPOSTER),
            COMPOST_READY
        );
    }

    /// 漏斗每格都有东西、但都没到堆叠上限时，官方先 `removeItem`（这一步就把桶清空），
    /// 再发现漏斗放不进骨粉。骨粉留在临时容器里，容器用完就没了。
    #[test]
    fn a_hopper_without_room_for_bone_meal_still_empties_the_composter() {
        let composter_pos = HOPPER.above();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(COMPOST_READY));
        level.set_container(HOPPER, vec![ItemStack::new(items::DIRT, 10); 5]);
        let mut ticks = BlockTicks::new();
        let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::Down);

        world.tick_block_entities();
        assert_eq!(slot(&mut world, HOPPER, 0), ItemStack::new(items::DIRT, 10));
        assert_eq!(world.block_state(composter_pos).get(LEVEL_COMPOSTER), 0);
    }

    #[test]
    fn reaching_level_seven_schedules_the_ready_tick() {
        let composter_pos = HOPPER.below();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(6));
        let mut slots = empty_slots(5);
        slots[0] = ItemStack::new(items::CAKE, 1);
        level.set_container(HOPPER, slots);
        let mut ticks = BlockTicks::new();
        {
            let mut world = placed_hopper(&mut level, &mut ticks, HOPPER, Direction::Down);
            world.tick_block_entities();
            assert_eq!(
                world.block_state(composter_pos).get(LEVEL_COMPOSTER),
                COMPOST_MAX_LEVEL
            );
        }
        let pending: Vec<_> = ticks.pending().collect();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].pos, composter_pos);
        assert_eq!(pending[0].trigger_tick, 20);
    }

    #[test]
    fn a_dropper_composts_into_the_composter_it_faces() {
        let composter_pos = HOPPER.below();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(1));
        let mut slots = empty_slots(9);
        slots[0] = ItemStack::new(items::CAKE, 2);
        level.set_container(HOPPER, slots);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.set_block(
            HOPPER,
            BlockState(blocks::DROPPER).with(FACING, Direction::Down),
            crate::world::UPDATE_ALL,
        );
        let state = world.block_state(HOPPER);
        dispense_from(&mut world, HOPPER, state);
        assert_eq!(slot(&mut world, HOPPER, 0), ItemStack::new(items::CAKE, 1));
        assert_eq!(world.block_state(composter_pos).get(LEVEL_COMPOSTER), 2);
        // 只抽了选槽的 `nextInt(1)`，蛋糕不加随机数，也没有抛物品的那一串。
        let twin = LegacyRandom::new(0);
        twin.next_int(1);
        assert_eq!(world.random().seed(), twin.seed());
    }

    #[test]
    fn a_dropper_facing_a_full_composter_keeps_the_item() {
        let composter_pos = HOPPER.below();
        let mut level = TestLevel::new();
        level.set(composter_pos, composter(COMPOST_MAX_LEVEL));
        let mut slots = empty_slots(9);
        slots[2] = ItemStack::new(items::BREAD, 3);
        level.set_container(HOPPER, slots);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.set_block(
            HOPPER,
            BlockState(blocks::DROPPER).with(FACING, Direction::Down),
            crate::world::UPDATE_ALL,
        );
        let state = world.block_state(HOPPER);
        dispense_from(&mut world, HOPPER, state);
        assert_eq!(slot(&mut world, HOPPER, 2), ItemStack::new(items::BREAD, 3));
        assert_eq!(
            world.block_state(composter_pos).get(LEVEL_COMPOSTER),
            COMPOST_MAX_LEVEL
        );
        let twin = LegacyRandom::new(0);
        twin.next_int(1);
        assert_eq!(world.random().seed(), twin.seed());
    }
}
