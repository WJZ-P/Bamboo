//! 熔炉三兄弟、酿造台和架子的容器。
//!
//! 熔炉和酿造台是官方的 `WorldlyContainer`：漏斗和投掷器按面选槽，再过
//! `canPlaceItem` 和 `canTakeItemThroughFace`。物品在方块实体上；方块 oracle 不建实体，
//! 槽位在关卡的容器表里，规则同一套。架子是 `ListBackedContainer`，不是 `WorldlyContainer`，
//! 三格都能用。
//!
//! 熔炉的 `setItem` 只有原料格换成另一种物品才 `setChanged`，`removeItem` 拿走东西才通知。
//! 燃料格和产物格写回去不通知：漏斗吸不走时会把物品放回，这一下不能再叫醒比较器。
//! 酿造台和架子按自己的 `setItem` / `removeItem`：写过就通知，拿走非空才通知。
//! 一次回调里这些通知合并到结束时发一次。漏斗每次只动一个物品，结束时读到的就是当时的内容。
//! 漏斗和投掷器成功之后还会再调一次 `setChanged`，那一下在它们自己的代码里。
//!
//! Furnace family and brewing stands are vanilla `WorldlyContainer`s: hoppers and droppers
//! pick slots by face, then apply `canPlaceItem` and `canTakeItemThroughFace`. Items live on
//! the block entity; the block oracle stores slots in the level table and uses the same rules.
//! A shelf is a `ListBackedContainer`, not a `WorldlyContainer`, so all three slots are open.
//!
//! A furnace `setItem` calls `setChanged` only when the input slot changes to a different item.
//! `removeItem` notifies when it takes something. Putting fuel or a result back does not:
//! a hopper that fails to pull restores the stack, and that write must not wake the comparator.
//! Brewing stands and shelves follow their own `setItem` / `removeItem`: a write notifies, a
//! non-empty removal notifies. Those calls are merged into one update at the end of the
//! callback. A hopper moves one item, so the slots at the end are what that call would read.
//! A hopper or dropper calls `setChanged` again after a successful move, in its own code.

use std::cell::Cell;

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, items, tags::item_tags};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_recipes::{
    is_brewing_fuel, is_brewing_potion_input, is_brewing_reagent, is_cooking_fuel,
};

use crate::{
    behaviour::machines::{BrewingStandEntity, FurnaceEntity},
    block_entity::BlockEntity,
    container::{
        Container, copy_with_count, redstone_signal, same_item_same_components, stack_limit,
    },
    state::item_is_tagged,
    world::World,
};

/// 熔炉原料、燃料、产物。官方 `SLOTS_FOR_UP`、`SLOTS_FOR_SIDES`、`SLOTS_FOR_DOWN`。
///
/// Furnace input, fuel and result. Vanilla `SLOTS_FOR_UP`, `SLOTS_FOR_SIDES`, `SLOTS_FOR_DOWN`.
const FURNACE_SLOTS: usize = 3;
const FURNACE_UP: &[usize] = &[0];
const FURNACE_SIDE: &[usize] = &[1];
const FURNACE_DOWN: &[usize] = &[2, 1];

/// 酿造台瓶子、材料、燃料。官方先看上面，再看下面，最后才是侧面。
///
/// Brewing bottles, ingredient and fuel. Vanilla checks up, then down, then the sides.
const BREWING_SLOTS: usize = 5;
const BREWING_UP: &[usize] = &[3];
const BREWING_DOWN: &[usize] = &[0, 1, 2, 3];
const BREWING_SIDE: &[usize] = &[0, 1, 2, 4];

/// 架子三格，官方 `ShelfBlockEntity.MAX_ITEMS`。
///
/// A shelf has three slots, vanilla `ShelfBlockEntity.MAX_ITEMS`.
const SHELF_SLOTS: usize = 3;

/// 熔炉、酿造台或架子时把容器交给 `f`。不是这三种方块时返回 `None`。
///
/// Hands the container to `f` for a furnace, brewing stand or shelf. Returns `None` for
/// any other block.
pub(crate) fn try_with_container(
    world: &mut World,
    pos: BlockPos,
    f: &mut dyn FnMut(&mut dyn Container),
) -> Option<bool> {
    let state = world.block_state(pos);
    if state.is_a(BlockAncestor::AbstractFurnaceBlock) {
        return Some(with_furnace(world, pos, f));
    }
    if state.is_a(BlockAncestor::BrewingStandBlock) {
        return Some(with_brewing(world, pos, f));
    }
    if state.is_a(BlockAncestor::ShelfBlock) {
        return Some(with_shelf(world, pos, f));
    }
    None
}

/// 官方 `AbstractFurnaceBlockEntity.canPlaceItem`。
///
/// Vanilla `AbstractFurnaceBlockEntity.canPlaceItem`.
pub(crate) fn furnace_can_place(items: &[ItemStack], slot: usize, stack: &ItemStack) -> bool {
    if slot == 2 {
        return false;
    }
    if slot != 1 {
        return true;
    }
    let fuel = items.get(1);
    is_cooking_fuel(stack)
        || stack.item == items::BUCKET && fuel.is_none_or(|fuel| fuel.item != items::BUCKET)
}

/// 官方 `canTakeItemThroughFace`：只有下面的燃料格要看 `furnace_fuel_bottom_takeable`。
///
/// Vanilla `canTakeItemThroughFace`: only the fuel slot from below checks
/// `furnace_fuel_bottom_takeable`.
pub(crate) fn furnace_can_take(slot: usize, stack: &ItemStack, direction: Direction) -> bool {
    if direction == Direction::Down && slot == 1 {
        item_is_tagged(stack.item, item_tags::FURNACE_FUEL_BOTTOM_TAKEABLE)
    } else {
        true
    }
}

fn furnace_face(direction: Direction) -> &'static [usize] {
    match direction {
        Direction::Down => FURNACE_DOWN,
        Direction::Up => FURNACE_UP,
        _ => FURNACE_SIDE,
    }
}

/// 官方 `BrewingStandBlockEntity.canPlaceItem`。`level` 在这里总是有的。
///
/// Vanilla `BrewingStandBlockEntity.canPlaceItem`. `level` is always present here.
pub(crate) fn brewing_can_place(items: &[ItemStack], slot: usize, stack: &ItemStack) -> bool {
    if slot == 4 {
        return is_brewing_fuel(stack);
    }
    if slot == 3 {
        return is_brewing_reagent(stack.item);
    }
    items
        .get(slot)
        .is_some_and(|current| is_brewing_potion_input(stack.item) && current.is_empty())
}

/// 官方 `canTakeItemThroughFace`：材料格只能取出玻璃瓶。
///
/// Vanilla `canTakeItemThroughFace`: the ingredient slot only releases a glass bottle.
pub(crate) fn brewing_can_take(slot: usize, stack: &ItemStack) -> bool {
    slot != 3 || stack.item == items::GLASS_BOTTLE
}

fn brewing_face(direction: Direction) -> &'static [usize] {
    match direction {
        Direction::Up => BREWING_UP,
        Direction::Down => BREWING_DOWN,
        _ => BREWING_SIDE,
    }
}

/// 官方 `BlockEntity.setChanged` 里通知比较器的那一步。方块是空气时不通知。
///
/// The comparator half of vanilla `BlockEntity.setChanged`. Air does not notify.
pub(crate) fn mark_container_changed(world: &mut World, pos: BlockPos) {
    let state = world.block_state(pos);
    if state.is_air() {
        return;
    }
    world.update_neighbour_for_output_signal(pos, state.block());
}

/// 物品没变的安静刻通知。`quiet_only` 为假时和 [`mark_container_changed`] 一样；
/// 只给证明测试用，见 `block_entity_sleep` 的证明测试。
///
/// The quiet, item-unchanged notification. With `quiet_only = false` this is
/// [`mark_container_changed`]; only the proof test switches the quiet half off.
pub(crate) fn quiet_mark_container_changed(world: &mut World, pos: BlockPos, quiet_only: bool) {
    #[cfg(test)]
    if quiet_only && !crate::block_entity_sleep::quiet_changed_enabled() {
        return;
    }
    let _ = quiet_only;
    mark_container_changed(world, pos);
}

/// 熔炉方块实体上的三格，给比较器读。没有实体时用槽位表。
///
/// The three furnace slots a comparator reads. Without an entity, the slot table is used.
pub(crate) fn furnace_signal(world: &World, pos: BlockPos) -> i32 {
    if let Some(entity) = world.block_entity::<FurnaceEntity>(pos) {
        return redstone_signal(entity.items().iter(), FURNACE_SLOTS);
    }
    signal_from_slots(world, pos, FURNACE_SLOTS)
}

/// 酿造台的五格。
///
/// The brewing stand's five slots.
pub(crate) fn brewing_signal(world: &World, pos: BlockPos) -> i32 {
    if let Some(entity) = world.block_entity::<BrewingStandEntity>(pos) {
        return redstone_signal(entity.items().iter(), BREWING_SLOTS);
    }
    signal_from_slots(world, pos, BREWING_SLOTS)
}

fn signal_from_slots(world: &World, pos: BlockPos, len: usize) -> i32 {
    let slots = world.slots_at(pos);
    if slots.len() < len {
        return 0;
    }
    redstone_signal(slots.iter().take(len), len)
}

/// 熔炉或酿造台方块实体里的物品。别的实体返回 `None`。
///
/// Items stored on a furnace or brewing-stand block entity. Other entities return `None`.
pub fn entity_container_items(entity: &dyn BlockEntity) -> Option<Vec<ItemStack>> {
    if let Some(furnace) = entity.as_any().downcast_ref::<FurnaceEntity>() {
        return Some(furnace.items().to_vec());
    }
    entity
        .as_any()
        .downcast_ref::<BrewingStandEntity>()
        .map(|stand| stand.items().to_vec())
}

fn with_furnace(world: &mut World, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
    if let Some(notify) = run_furnace(world, pos, f) {
        if notify {
            mark_container_changed(world, pos);
        }
        return true;
    }
    // 正在 tick 的实体拿不出来，不能再造一个。
    // The entity being ticked cannot be taken out, so do not create a second one.
    if world.has_block_entity(pos) {
        return false;
    }
    if with_furnace_slots(world, pos, f) {
        return true;
    }
    let state = world.block_state(pos);
    let Some(kind) = FurnaceEntity::kind_of(state.class()) else {
        return false;
    };
    world.set_block_entity(Box::new(FurnaceEntity::new(pos, kind)));
    if let Some(notify) = run_furnace(world, pos, f) {
        if notify {
            mark_container_changed(world, pos);
        }
        return true;
    }
    false
}

fn run_furnace(
    world: &mut World,
    pos: BlockPos,
    f: &mut dyn FnMut(&mut dyn Container),
) -> Option<bool> {
    let notify = Cell::new(false);
    let found = world.with_block_entity::<FurnaceEntity, _>(pos, |entity, _| {
        f(&mut FurnaceView {
            entity,
            notify: &notify,
        });
    });
    found.map(|_| notify.get())
}

/// 槽位表上的熔炉。通知规则和方块实体一样，不看“格子变没变”。
///
/// A furnace stored in the slot table. The notify rule matches the block entity:
/// a changed slot is not enough.
fn with_furnace_slots(
    world: &mut World,
    pos: BlockPos,
    f: &mut dyn FnMut(&mut dyn Container),
) -> bool {
    let notify = Cell::new(false);
    let mut ran = false;
    let ok = world.with_slots(pos, FURNACE_SLOTS, &mut |slots| {
        ran = true;
        f(&mut SlotView {
            kind: MachineKind::Furnace,
            slots,
            furnace_notify: Some(&notify),
        });
    });
    if ok && ran && notify.get() {
        mark_container_changed(world, pos);
    }
    ok && ran
}

fn with_brewing(world: &mut World, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
    if let Some(changed) = run_brewing(world, pos, f) {
        if changed {
            mark_container_changed(world, pos);
        }
        return true;
    }
    if world.has_block_entity(pos) {
        return false;
    }
    if with_slots(world, pos, BREWING_SLOTS, &mut |slots| {
        f(&mut SlotView {
            kind: MachineKind::Brewing,
            slots,
            furnace_notify: None,
        });
    }) {
        return true;
    }
    world.set_block_entity(Box::new(BrewingStandEntity::new(pos)));
    if let Some(changed) = run_brewing(world, pos, f) {
        if changed {
            mark_container_changed(world, pos);
        }
        return true;
    }
    false
}

fn run_brewing(
    world: &mut World,
    pos: BlockPos,
    f: &mut dyn FnMut(&mut dyn Container),
) -> Option<bool> {
    let mut changed = false;
    let found = world.with_block_entity::<BrewingStandEntity, _>(pos, |entity, _| {
        let before = entity.items().clone();
        f(&mut BrewingView { entity });
        changed = entity.items() != &before;
    });
    found.map(|_| changed)
}

fn with_shelf(world: &mut World, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
    with_slots(world, pos, SHELF_SLOTS, &mut |slots| {
        f(&mut ShelfView { slots });
    })
}

/// 槽位变了才通知。只读的访问不该让比较器重新排队。
///
/// Notify only when the slots change. A read must not schedule the comparator again.
fn with_slots(
    world: &mut World,
    pos: BlockPos,
    len: usize,
    f: &mut dyn FnMut(&mut [ItemStack]),
) -> bool {
    let mut changed = false;
    let mut ran = false;
    let ok = world.with_slots(pos, len, &mut |slots| {
        ran = true;
        let before = slots.to_vec();
        f(slots);
        changed = slots != before.as_slice();
    });
    if ok && ran && changed {
        mark_container_changed(world, pos);
    }
    ok && ran
}

fn limit_stack(mut stack: ItemStack) -> ItemStack {
    if stack.is_empty() {
        return ItemStack::empty();
    }
    let limit = stack_limit(&stack);
    if stack.count > limit {
        stack.count = limit;
    }
    stack
}

fn split_slot(slots: &mut [ItemStack], slot: usize, count: i32) -> ItemStack {
    let Some(current) = slots.get_mut(slot) else {
        return ItemStack::empty();
    };
    if current.is_empty() || count <= 0 {
        return ItemStack::empty();
    }
    let taken_count = count.min(current.count);
    let taken = copy_with_count(current, taken_count);
    current.count -= taken_count;
    if current.count <= 0 {
        *current = ItemStack::empty();
    }
    taken
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MachineKind {
    Furnace,
    Brewing,
}

struct FurnaceView<'a> {
    entity: &'a mut FurnaceEntity,
    /// `setItem` / `removeItem` 里要不要 `setChanged`。
    ///
    /// Whether `setItem` / `removeItem` should call `setChanged`.
    notify: &'a Cell<bool>,
}

impl Container for FurnaceView<'_> {
    fn size(&self) -> usize {
        FURNACE_SLOTS
    }

    fn get(&self, slot: usize) -> ItemStack {
        self.entity.item(slot).clone()
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        if slot >= FURNACE_SLOTS {
            return;
        }
        // 官方 `setItem` 在截断数量之前比 `same`，只有原料格换成别的物品才 `setChanged`。
        // Vanilla `setItem` compares `same` before limiting the count, and `setChanged` runs
        // only when the input slot changes to a different item.
        let old = self.entity.item(slot);
        let same = !stack.is_empty() && same_item_same_components(old, &stack);
        self.entity.set_item(slot, stack);
        if slot == 0 && !same {
            self.notify.set(true);
        }
    }

    fn remove_item(&mut self, slot: usize, count: i32) -> ItemStack {
        // 官方 `removeItem` 不经过 `setItem`，原料格的烹饪进度不清零。拿走东西就 `setChanged`。
        // Vanilla `removeItem` does not go through `setItem`, so the input slot keeps its cook
        // time. Taking something calls `setChanged`.
        let taken = self.entity.remove_item(slot, count);
        if !taken.is_empty() {
            self.notify.set(true);
        }
        taken
    }

    fn is_worldly(&self) -> bool {
        true
    }

    fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        furnace_can_place(self.entity.items(), slot, stack)
    }

    fn slots_for_face(&self, direction: Direction) -> Option<&[usize]> {
        Some(furnace_face(direction))
    }

    fn can_place_item_through_face(
        &self,
        slot: usize,
        stack: &ItemStack,
        _direction: Option<Direction>,
    ) -> bool {
        self.can_place_item(slot, stack)
    }

    fn can_take_item_through_face(
        &self,
        slot: usize,
        stack: &ItemStack,
        direction: Direction,
    ) -> bool {
        furnace_can_take(slot, stack, direction)
    }
}

struct BrewingView<'a> {
    entity: &'a mut BrewingStandEntity,
}

impl Container for BrewingView<'_> {
    fn size(&self) -> usize {
        BREWING_SLOTS
    }

    fn get(&self, slot: usize) -> ItemStack {
        self.entity.item(slot).clone()
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        if slot < BREWING_SLOTS {
            self.entity.set_item(slot, stack);
        }
    }

    fn remove_item(&mut self, slot: usize, count: i32) -> ItemStack {
        self.entity.remove_item(slot, count)
    }

    fn is_worldly(&self) -> bool {
        true
    }

    fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        brewing_can_place(self.entity.items(), slot, stack)
    }

    fn slots_for_face(&self, direction: Direction) -> Option<&[usize]> {
        Some(brewing_face(direction))
    }

    fn can_place_item_through_face(
        &self,
        slot: usize,
        stack: &ItemStack,
        _direction: Option<Direction>,
    ) -> bool {
        self.can_place_item(slot, stack)
    }

    fn can_take_item_through_face(
        &self,
        slot: usize,
        stack: &ItemStack,
        _direction: Direction,
    ) -> bool {
        brewing_can_take(slot, stack)
    }
}

struct SlotView<'a> {
    kind: MachineKind,
    slots: &'a mut [ItemStack],
    /// 熔炉槽位表上的 `setChanged` 标记。酿造台走外层的“格子变了”。
    ///
    /// Furnace slot-table `setChanged` flag. A brewing stand uses the outer "slots changed" check.
    furnace_notify: Option<&'a Cell<bool>>,
}

impl Container for SlotView<'_> {
    fn size(&self) -> usize {
        self.slots.len()
    }

    fn get(&self, slot: usize) -> ItemStack {
        self.slots[slot].clone()
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        if slot >= self.slots.len() {
            return;
        }
        let same = !stack.is_empty() && same_item_same_components(&self.slots[slot], &stack);
        self.slots[slot] = limit_stack(stack);
        if self.kind == MachineKind::Furnace
            && slot == 0
            && !same
            && let Some(notify) = self.furnace_notify
        {
            notify.set(true);
        }
    }

    fn remove_item(&mut self, slot: usize, count: i32) -> ItemStack {
        let taken = split_slot(self.slots, slot, count);
        if self.kind == MachineKind::Furnace
            && !taken.is_empty()
            && let Some(notify) = self.furnace_notify
        {
            notify.set(true);
        }
        taken
    }

    fn is_worldly(&self) -> bool {
        true
    }

    fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        match self.kind {
            MachineKind::Furnace => furnace_can_place(self.slots, slot, stack),
            MachineKind::Brewing => brewing_can_place(self.slots, slot, stack),
        }
    }

    fn slots_for_face(&self, direction: Direction) -> Option<&[usize]> {
        Some(match self.kind {
            MachineKind::Furnace => furnace_face(direction),
            MachineKind::Brewing => brewing_face(direction),
        })
    }

    fn can_place_item_through_face(
        &self,
        slot: usize,
        stack: &ItemStack,
        _direction: Option<Direction>,
    ) -> bool {
        self.can_place_item(slot, stack)
    }

    fn can_take_item_through_face(
        &self,
        slot: usize,
        stack: &ItemStack,
        direction: Direction,
    ) -> bool {
        match self.kind {
            MachineKind::Furnace => furnace_can_take(slot, stack, direction),
            MachineKind::Brewing => brewing_can_take(slot, stack),
        }
    }
}

struct ShelfView<'a> {
    slots: &'a mut [ItemStack],
}

impl Container for ShelfView<'_> {
    fn size(&self) -> usize {
        self.slots.len()
    }

    fn get(&self, slot: usize) -> ItemStack {
        self.slots[slot].clone()
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        if slot < self.slots.len() {
            self.slots[slot] = limit_stack(stack);
        }
    }

    fn remove_item(&mut self, slot: usize, count: i32) -> ItemStack {
        split_slot(self.slots, slot, count)
    }

    fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        // 官方 `ListBackedContainer.canPlaceItem`：这一格是空的，或者还没到堆叠上限。
        // 不要求和格子里是同一种物品；合并不成的情况由漏斗自己判断。
        //
        // Vanilla `ListBackedContainer.canPlaceItem`: the slot is empty, or it is under the
        // stack limit. It does not require the same item; a failed merge is the hopper's check.
        self.slots
            .get(slot)
            .is_some_and(|current| current.is_empty() || current.count < stack_limit(stack))
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Direction};
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockState, BlockTicks, LevelSideState, LevelTables,
        dispenser::dispense_from,
        hopper::HopperEntity,
        state::{FACING, FACING_HOPPER, HORIZONTAL_FACING, LIT},
        testing::TestLevel,
        world::UPDATE_ALL,
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn world_of<'a>(level: &'a mut TestLevel, ticks: &'a mut BlockTicks) -> World<'a> {
        World::new(level, ticks)
    }

    fn place(world: &mut World, pos: BlockPos, state: BlockState) {
        world.set_block(pos, state, UPDATE_ALL);
    }

    fn slot(world: &mut World, pos: BlockPos, index: usize) -> ItemStack {
        let mut found = ItemStack::empty();
        world.with_container(pos, &mut |container| {
            found = container.get(index);
        });
        found
    }

    fn assert_furnace_rules(world: &mut World, pos: BlockPos) {
        for direction in Direction::ALL {
            let mut faces = Vec::new();
            let mut worldly = false;
            world.with_container(pos, &mut |container| {
                worldly = container.is_worldly();
                faces = container.slots_for_face(direction).unwrap().to_vec();
            });
            assert!(worldly);
            let expected: &[usize] = match direction {
                Direction::Down => FURNACE_DOWN,
                Direction::Up => FURNACE_UP,
                _ => FURNACE_SIDE,
            };
            assert_eq!(faces, expected);
        }

        let bread = ItemStack::new(items::BREAD, 1);
        let coal = ItemStack::new(items::COAL, 1);
        let bucket = ItemStack::new(items::BUCKET, 1);
        let water = ItemStack::new(items::WATER_BUCKET, 1);
        let lava = ItemStack::new(items::LAVA_BUCKET, 1);
        world.with_container(pos, &mut |container| {
            assert!(container.can_place_item(0, &bread));
            assert!(!container.can_place_item(2, &bread));
            assert!(container.can_place_item(1, &coal));
            assert!(container.can_place_item(1, &lava));
            assert!(container.can_place_item(1, &bucket));
            assert!(!container.can_place_item(1, &bread));
            assert!(!container.can_place_item(1, &water));
            assert!(container.can_place_item_through_face(0, &bread, Some(Direction::Up)));
            assert!(!container.can_place_item_through_face(1, &bread, Some(Direction::North)));
            assert!(!container.can_place_item_through_face(2, &bread, Some(Direction::Down)));
            assert!(container.can_place_item_through_face(1, &coal, Some(Direction::Down)));
            assert!(container.can_take_item_through_face(2, &bread, Direction::Down));
            assert!(container.can_take_item_through_face(1, &water, Direction::Down));
            assert!(container.can_take_item_through_face(1, &bucket, Direction::Down));
            assert!(!container.can_take_item_through_face(1, &coal, Direction::Down));
            assert!(!container.can_take_item_through_face(1, &lava, Direction::Down));
            assert!(container.can_take_item_through_face(1, &lava, Direction::Up));
            assert!(container.can_take_item_through_face(1, &coal, Direction::North));
            assert!(container.can_take_item_through_face(0, &bread, Direction::Up));
            assert!(container.can_take_item_through_face(2, &bread, Direction::North));
        });
    }

    #[test]
    fn furnace_faces_follow_the_entity_and_the_slot_table() {
        for block in [blocks::FURNACE, blocks::BLAST_FURNACE, blocks::SMOKER] {
            let mut level = TestLevel::new();
            let mut ticks = BlockTicks::new();
            {
                let mut world = world_of(&mut level, &mut ticks);
                place(&mut world, POS, BlockState(block));
                assert_furnace_rules(&mut world, POS);
            }
            let mut level = TestLevel::new();
            level.set(POS, BlockState(block));
            let mut world = world_of(&mut level, &mut ticks);
            assert!(!world.has_block_entity(POS));
            assert_furnace_rules(&mut world, POS);
        }
    }

    #[test]
    fn a_second_empty_bucket_does_not_enter_the_fuel_slot() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = world_of(&mut level, &mut ticks);
        place(&mut world, POS, BlockState(blocks::FURNACE));
        let bucket = ItemStack::new(items::BUCKET, 1);
        world.with_container(POS, &mut |container| {
            let added = crate::hopper::add_item(container, bucket.clone(), Some(Direction::North));
            assert!(added.remaining.is_empty());
        });
        world.with_container(POS, &mut |container| {
            assert!(!container.can_place_item(1, &bucket));
            let added = crate::hopper::add_item(container, bucket.clone(), Some(Direction::North));
            assert_eq!(added.remaining, bucket);
        });
        assert_eq!(slot(&mut world, POS, 1), bucket);
    }

    #[test]
    fn brewing_faces_accept_reagent_fuel_and_empty_bottle_slots() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = world_of(&mut level, &mut ticks);
        place(&mut world, POS, BlockState(blocks::BREWING_STAND));
        let bottle = ItemStack::new(items::GLASS_BOTTLE, 1);
        let powder = ItemStack::new(items::BLAZE_POWDER, 1);
        let wart = ItemStack::new(items::NETHER_WART, 1);
        for direction in Direction::ALL {
            let mut faces = Vec::new();
            world.with_container(POS, &mut |container| {
                assert!(container.is_worldly());
                faces = container.slots_for_face(direction).unwrap().to_vec();
            });
            let expected: &[usize] = match direction {
                Direction::Up => BREWING_UP,
                Direction::Down => BREWING_DOWN,
                _ => BREWING_SIDE,
            };
            assert_eq!(faces, expected);
        }
        world.with_container(POS, &mut |container| {
            assert!(container.can_place_item(3, &wart));
            assert!(container.can_place_item(4, &powder));
            assert!(!container.can_place_item(4, &wart));
            assert!(container.can_place_item(0, &bottle));
            assert!(!container.can_place_item(0, &powder));
            assert!(container.can_take_item_through_face(3, &bottle, Direction::Down));
            assert!(!container.can_take_item_through_face(3, &wart, Direction::Down));
            assert!(container.can_take_item_through_face(0, &bottle, Direction::Down));
            assert!(container.can_take_item_through_face(4, &powder, Direction::North));
        });
        world.with_container(POS, &mut |container| {
            let added = crate::hopper::add_item(container, bottle.clone(), Some(Direction::North));
            assert!(added.remaining.is_empty());
        });
        world.with_container(POS, &mut |container| {
            assert!(!container.can_place_item(0, &bottle));
            assert!(container.can_place_item(1, &bottle));
        });
    }

    #[test]
    fn a_hopper_under_a_brewing_stand_pulls_a_bottle_not_the_fuel() {
        let above = POS.above();
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::HOPPER));
        level.set_container(POS, vec![ItemStack::empty(); 5]);
        let mut ticks = BlockTicks::new();
        let mut world = world_of(&mut level, &mut ticks);
        place(&mut world, above, BlockState(blocks::BREWING_STAND));
        world.set_block_entity(Box::new(HopperEntity::new(POS)));
        world.with_block_entity::<BrewingStandEntity, _>(above, |entity, _| {
            entity.set_item(0, ItemStack::new(items::GLASS_BOTTLE, 1));
            entity.set_item(4, ItemStack::new(items::BLAZE_POWDER, 1));
            // 燃料余量大于 0，这一刻不会把烈焰粉消耗掉。
            // Fuel remaining above 0, so this tick does not consume the blaze powder.
            entity.set_data(1, 1);
        });

        world.tick_block_entities();

        assert_eq!(
            slot(&mut world, POS, 0),
            ItemStack::new(items::GLASS_BOTTLE, 1)
        );
        assert!(slot(&mut world, above, 0).is_empty());
        assert_eq!(
            slot(&mut world, above, 4),
            ItemStack::new(items::BLAZE_POWDER, 1)
        );
    }

    #[test]
    fn a_hopper_under_a_furnace_pulls_the_result_not_the_coal() {
        let above = POS.above();
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::HOPPER));
        level.set_container(POS, vec![ItemStack::empty(); 5]);
        let mut ticks = BlockTicks::new();
        let mut world = world_of(&mut level, &mut ticks);
        place(&mut world, above, BlockState(blocks::FURNACE));
        world.set_block_entity(Box::new(HopperEntity::new(POS)));
        world.with_block_entity::<FurnaceEntity, _>(above, |entity, _| {
            entity.write_item(1, ItemStack::new(items::COAL, 1));
            entity.write_item(2, ItemStack::new(items::IRON_INGOT, 1));
        });

        world.tick_block_entities();

        assert_eq!(
            slot(&mut world, POS, 0),
            ItemStack::new(items::IRON_INGOT, 1)
        );
        assert!(slot(&mut world, above, 2).is_empty());
        assert_eq!(slot(&mut world, above, 1), ItemStack::new(items::COAL, 1));
    }

    #[test]
    fn bread_fired_at_a_blast_furnace_stays_in_the_dropper() {
        let furnace = POS.relative(Direction::East);
        let mut level = TestLevel::new();
        let mut dropper = vec![ItemStack::empty(); 9];
        dropper[0] = ItemStack::new(items::BREAD, 1);
        level.set_container(POS, dropper);
        let mut ticks = BlockTicks::new();
        let mut world = world_of(&mut level, &mut ticks);
        place(
            &mut world,
            POS,
            BlockState(blocks::DROPPER).with(FACING, Direction::East),
        );
        place(&mut world, furnace, BlockState(blocks::BLAST_FURNACE));
        let before = world.random().clone();

        dispense_from(
            &mut world,
            POS,
            BlockState(blocks::DROPPER).with(FACING, Direction::East),
        );

        let once = before;
        once.next_int(1);
        assert_eq!(world.random().seed(), once.seed());
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::BREAD, 1));
        assert!(slot(&mut world, furnace, 0).is_empty());
        assert!(slot(&mut world, furnace, 1).is_empty());
        assert!(slot(&mut world, furnace, 2).is_empty());
    }

    #[test]
    fn a_dropper_puts_coal_into_the_blast_furnace_fuel_slot() {
        let furnace = POS.relative(Direction::East);
        let mut level = TestLevel::new();
        let mut dropper = vec![ItemStack::empty(); 9];
        dropper[0] = ItemStack::new(items::COAL, 1);
        level.set_container(POS, dropper);
        let mut ticks = BlockTicks::new();
        let mut world = world_of(&mut level, &mut ticks);
        place(
            &mut world,
            POS,
            BlockState(blocks::DROPPER).with(FACING, Direction::East),
        );
        place(&mut world, furnace, BlockState(blocks::BLAST_FURNACE));

        dispense_from(
            &mut world,
            POS,
            BlockState(blocks::DROPPER).with(FACING, Direction::East),
        );

        assert!(slot(&mut world, POS, 0).is_empty());
        assert_eq!(slot(&mut world, furnace, 1), ItemStack::new(items::COAL, 1));
    }

    #[test]
    fn a_dropper_below_a_furnace_uses_the_bottom_face() {
        let furnace = POS.above();
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let facing = Direction::Up;
        let state = BlockState(blocks::DROPPER).with(FACING, facing);
        {
            let mut world = world_of(&mut level, &mut ticks);
            place(&mut world, POS, state);
            place(&mut world, furnace, BlockState(blocks::FURNACE));
        }
        let mut coal = vec![ItemStack::empty(); 9];
        coal[0] = ItemStack::new(items::COAL, 1);
        level.set_container(POS, coal);
        let mut world = world_of(&mut level, &mut ticks);
        dispense_from(&mut world, POS, state);
        assert!(slot(&mut world, POS, 0).is_empty());
        assert_eq!(slot(&mut world, furnace, 1), ItemStack::new(items::COAL, 1));
        assert!(slot(&mut world, furnace, 2).is_empty());

        world.with_container(POS, &mut |container| {
            container.set(0, ItemStack::new(items::BREAD, 1));
        });
        let before = world.random().clone();
        dispense_from(&mut world, POS, state);
        let once = before;
        once.next_int(1);
        assert_eq!(world.random().seed(), once.seed());
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::BREAD, 1));
        assert_eq!(slot(&mut world, furnace, 1), ItemStack::new(items::COAL, 1));
    }

    #[test]
    fn hoppers_push_into_the_furnace_face_they_point_at() {
        let furnace = POS;
        let side = furnace.west();
        let above = furnace.above();
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        {
            let mut world = world_of(&mut level, &mut ticks);
            place(&mut world, furnace, BlockState(blocks::SMOKER));
            place(
                &mut world,
                side,
                BlockState(blocks::HOPPER).with(FACING_HOPPER, Direction::East),
            );
            place(
                &mut world,
                above,
                BlockState(blocks::HOPPER).with(FACING_HOPPER, Direction::Down),
            );
        }
        level.insert_block_entity(Box::new(HopperEntity::new(side)));
        level.insert_block_entity(Box::new(HopperEntity::new(above)));
        let mut side_slots = vec![ItemStack::empty(); 5];
        side_slots[0] = ItemStack::new(items::COAL, 1);
        level.set_container(side, side_slots);
        let mut above_slots = vec![ItemStack::empty(); 5];
        above_slots[0] = ItemStack::new(items::BREAD, 1);
        level.set_container(above, above_slots);
        level.set(furnace.east().below(), BlockState(blocks::STONE));
        level.set(
            furnace.east(),
            BlockState(blocks::COMPARATOR).with(HORIZONTAL_FACING, Direction::West),
        );
        let mut world = world_of(&mut level, &mut ticks);
        let comparator = furnace.east();
        let comparator_block = world.block_state(comparator).block();
        assert!(!world.has_scheduled_tick(comparator, comparator_block));

        world.tick_block_entities();

        assert!(slot(&mut world, side, 0).is_empty());
        assert!(slot(&mut world, above, 0).is_empty());
        assert_eq!(
            slot(&mut world, furnace, 0),
            ItemStack::new(items::BREAD, 1)
        );
        assert_eq!(slot(&mut world, furnace, 1), ItemStack::new(items::COAL, 1));
        assert!(world.has_scheduled_tick(comparator, comparator_block));
    }

    #[test]
    fn a_failed_furnace_pull_does_not_schedule_the_comparator() {
        // 漏斗吸不走最后一件产物时，官方 `removeItem` 通知的是空格子（信号 0，比较器本来就是 0，不排刻），
        // 再 `setItem` 放回产物格。产物格的 `setItem` 不 `setChanged`，所以比较器仍然不排刻。
        //
        // When a hopper fails to pull the last result, vanilla `removeItem` notifies the empty
        // slot (signal 0, and the comparator is already 0, so no tick). `setItem` then puts the
        // result back. That `setItem` does not `setChanged`, so the comparator stays unscheduled.
        let furnace = POS.above();
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::HOPPER));
        let mut hopper = vec![ItemStack::new(items::COBBLESTONE, 1); 5];
        hopper[0] = ItemStack::new(items::COBBLESTONE, 64);
        level.set_container(POS, hopper);
        let mut ticks = BlockTicks::new();
        {
            let mut world = world_of(&mut level, &mut ticks);
            place(&mut world, furnace, BlockState(blocks::FURNACE));
        }
        level.insert_block_entity(Box::new(HopperEntity::new(POS)));
        level
            .block_entities_table_mut()
            .get_mut::<FurnaceEntity>(furnace)
            .unwrap()
            .write_item(2, ItemStack::new(items::IRON_INGOT, 1));
        let comparator = furnace.west();
        level.set(comparator.below(), BlockState(blocks::STONE));
        level.set(
            comparator,
            BlockState(blocks::COMPARATOR).with(HORIZONTAL_FACING, Direction::East),
        );
        let mut world = world_of(&mut level, &mut ticks);
        let comparator_block = world.block_state(comparator).block();

        world.tick_block_entities();

        assert_eq!(
            slot(&mut world, furnace, 2),
            ItemStack::new(items::IRON_INGOT, 1)
        );
        assert_eq!(
            slot(&mut world, POS, 1),
            ItemStack::new(items::COBBLESTONE, 1)
        );
        assert!(!world.has_scheduled_tick(comparator, comparator_block));
    }

    #[test]
    fn a_failed_pull_from_the_slot_table_does_not_schedule_the_comparator() {
        let furnace = POS.above();
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::HOPPER));
        level.set(furnace, BlockState(blocks::BLAST_FURNACE));
        let mut hopper = vec![ItemStack::new(items::COBBLESTONE, 1); 5];
        hopper[0] = ItemStack::new(items::COBBLESTONE, 64);
        level.set_container(POS, hopper);
        let mut stored = vec![ItemStack::empty(); 3];
        stored[2] = ItemStack::new(items::IRON_INGOT, 1);
        level.set_container(furnace, stored);
        let mut ticks = BlockTicks::new();
        let comparator = furnace.west();
        level.set(comparator.below(), BlockState(blocks::STONE));
        level.set(
            comparator,
            BlockState(blocks::COMPARATOR).with(HORIZONTAL_FACING, Direction::East),
        );
        let mut world = world_of(&mut level, &mut ticks);
        assert!(!world.has_block_entity(furnace));
        world.set_block_entity(Box::new(HopperEntity::new(POS)));
        let comparator_block = world.block_state(comparator).block();

        world.tick_block_entities();

        assert_eq!(
            slot(&mut world, furnace, 2),
            ItemStack::new(items::IRON_INGOT, 1)
        );
        assert!(!world.has_scheduled_tick(comparator, comparator_block));
    }

    #[test]
    fn a_furnace_lighting_is_visible_to_the_comparator() {
        let furnace = POS;
        let comparator = furnace.west();
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        {
            let mut world = world_of(&mut level, &mut ticks);
            place(&mut world, furnace, BlockState(blocks::FURNACE));
        }
        level
            .block_entities_table_mut()
            .get_mut::<FurnaceEntity>(furnace)
            .unwrap()
            .write_item(0, ItemStack::new(items::RAW_IRON, 1));
        level
            .block_entities_table_mut()
            .get_mut::<FurnaceEntity>(furnace)
            .unwrap()
            .write_item(1, ItemStack::new(items::COAL, 1));
        // 比较器要立在完整方块上，否则邻居更新会把它拆掉。
        //
        // The comparator needs a full block under it, or the neighbor update removes it.
        level.set(comparator.below(), BlockState(blocks::STONE));
        level.set(
            comparator,
            BlockState(blocks::COMPARATOR).with(HORIZONTAL_FACING, Direction::East),
        );
        let mut world = world_of(&mut level, &mut ticks);
        let comparator_block = BlockState(blocks::COMPARATOR).block();
        assert!(!world.has_scheduled_tick(comparator, comparator_block));

        world.tick_block_entities();

        assert!(world.block_state(furnace).get(LIT));
        assert!(world.has_scheduled_tick(comparator, comparator_block));
        let signal =
            world
                .block_state(furnace)
                .analog_output_signal(&world, furnace, Direction::West);
        let entity = world.block_entity::<FurnaceEntity>(furnace).unwrap();
        assert_eq!(
            signal,
            redstone_signal(entity.items().iter(), FURNACE_SLOTS)
        );
        assert!(signal > 0);
    }

    #[test]
    fn items_placed_inside_with_block_entity_reach_the_comparator() {
        let furnace = POS;
        let comparator = furnace.west();
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        {
            let mut world = world_of(&mut level, &mut ticks);
            place(&mut world, furnace, BlockState(blocks::FURNACE));
        }
        level.set(comparator.below(), BlockState(blocks::STONE));
        level.set(
            comparator,
            BlockState(blocks::COMPARATOR).with(HORIZONTAL_FACING, Direction::East),
        );
        let mut world = world_of(&mut level, &mut ticks);
        let comparator_block = BlockState(blocks::COMPARATOR).block();
        assert!(!world.has_scheduled_tick(comparator, comparator_block));

        let held = world.with_block_entity::<FurnaceEntity, _>(furnace, |entity, world| {
            assert!(world.has_block_entity(furnace));
            assert!(world.block_entity::<FurnaceEntity>(furnace).is_none());
            entity.set_item(0, ItemStack::new(items::COBBLESTONE, 1));
            mark_container_changed(world, furnace);
            // 通知先排着。这时候去读会落到空的槽位表，比较器不会被排上。
            //
            // The notification is queued. Reading now would see the empty slot table and the
            // comparator would not be scheduled.
            assert!(!world.has_scheduled_tick(comparator, comparator_block));
        });
        assert!(held.is_some());

        assert!(world.has_scheduled_tick(comparator, comparator_block));
        let signal =
            world
                .block_state(furnace)
                .analog_output_signal(&world, furnace, Direction::West);
        let entity = world.block_entity::<FurnaceEntity>(furnace).unwrap();
        assert_eq!(
            signal,
            redstone_signal(entity.items().iter(), FURNACE_SLOTS)
        );
        assert!(signal > 0);
    }

    #[test]
    fn a_shelf_takes_any_item_and_reports_occupancy_from_the_back() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let state = BlockState(blocks::OAK_SHELF).with(HORIZONTAL_FACING, Direction::South);
        level.set(POS, state);
        let mut world = world_of(&mut level, &mut ticks);
        let dirt = ItemStack::new(items::DIRT, 1);
        world.with_container(POS, &mut |container| {
            assert!(!container.is_worldly());
            assert!(container.slots_for_face(Direction::Up).is_none());
            assert!(container.can_place_item(0, &dirt));
            let added = crate::hopper::add_item(container, dirt.clone(), None);
            assert!(added.remaining.is_empty());
        });
        assert_eq!(state.analog_output_signal(&world, POS, Direction::North), 1);
        assert_eq!(state.analog_output_signal(&world, POS, Direction::South), 0);
        world.with_slots(POS, SHELF_SLOTS, &mut |slots| {
            slots[1] = ItemStack::new(items::DIRT, 1);
            slots[2] = ItemStack::new(items::DIRT, 1);
        });
        assert_eq!(state.analog_output_signal(&world, POS, Direction::North), 7);
    }
}
