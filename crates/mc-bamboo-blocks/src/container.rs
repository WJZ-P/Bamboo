//! 物品栏，对应官方的 `Container` 和它的默认实现。
//!
//! 物品栏的内容存在 `mc-bamboo-game` 的 [`Region`](crate::Level) 或者 oracle 里，
//! 方块逻辑通过 [`Level::with_container`] 拿到一段能读能写的槽位表。这里只放两边共用的规则：
//! 堆叠上限、合并、取出，以及比较器读出来的模拟信号。
//!
//! 双箱在官方是 `CompoundContainer`，这里由 [`Combined`] 把两半的槽位表拼起来。

use mc_bamboo_core::Direction;
use mc_bamboo_data::{BlockAncestor, blocks, items::ITEMS};
use mc_bamboo_protocol::item::ItemStack;

/// `Container.getMaxStackSize()` 的默认值，方块实体都没有覆盖它。
pub const CONTAINER_STACK_LIMIT: i32 = 99;

/// `ItemStack.getMaxStackSize()`：物品自己的堆叠上限。
pub fn item_max(stack: &ItemStack) -> i32 {
    usize::try_from(stack.item)
        .ok()
        .and_then(|id| ITEMS.get(id))
        .map_or(1, |item| item.max_stack_size)
}

/// 官方 `Container.getMaxStackSize(ItemStack)`：容器上限和物品上限取小的。
pub fn stack_limit(stack: &ItemStack) -> i32 {
    CONTAINER_STACK_LIMIT.min(item_max(stack))
}

/// 官方 `ItemStack.isSameItemSameComponents`。空物品和空物品也算相同。
pub fn same_item_same_components(a: &ItemStack, b: &ItemStack) -> bool {
    a.item == b.item && same_components(a, b)
}

/// 官方 `DataComponentPatch` 的相等。补丁按类型排序后比较，值按字节比较。
pub fn same_components(a: &ItemStack, b: &ItemStack) -> bool {
    let mut added_a: Vec<(i32, &[u8])> = a.added.iter().map(|(k, v)| (*k, v.as_slice())).collect();
    let mut added_b: Vec<(i32, &[u8])> = b.added.iter().map(|(k, v)| (*k, v.as_slice())).collect();
    added_a.sort_unstable();
    added_b.sort_unstable();
    if added_a != added_b {
        return false;
    }
    let mut removed_a = a.removed.clone();
    let mut removed_b = b.removed.clone();
    removed_a.sort_unstable();
    removed_b.sort_unstable();
    removed_a == removed_b
}

/// 官方 `ItemStack.copyWithCount`。
pub fn copy_with_count(stack: &ItemStack, count: i32) -> ItemStack {
    if stack.is_empty() {
        return ItemStack::empty();
    }
    let mut copy = stack.clone();
    copy.count = count;
    copy
}

/// 官方 `Container.removeItem(slot, count)`，也就是 `ContainerHelper.removeItem`：
/// 拿出 `count` 个，剩下的留在原来的槽里，槽空了就换成空堆。
pub fn remove_item(container: &mut dyn Container, slot: usize, count: i32) -> ItemStack {
    let stack = container.get(slot);
    if stack.is_empty() || count <= 0 {
        return ItemStack::empty();
    }
    let taken = copy_with_count(&stack, count.min(stack.count));
    let left = stack.count - taken.count;
    container.set(
        slot,
        if left <= 0 {
            ItemStack::empty()
        } else {
            copy_with_count(&stack, left)
        },
    );
    taken
}

/// 官方 `Container`。只有这一层用得到的部分。
///
/// `slots_for_face` 返回 `Some` 表示它是 `WorldlyContainer`，漏斗要看这个面允许哪些槽。
/// 普通方块实体返回 `None`，也就是整张槽位表都能用；潜影盒返回 `Some`，见 [`ShulkerSlots`]。
pub trait Container {
    /// `getContainerSize`。
    fn size(&self) -> usize;

    /// `getItem`。
    fn get(&self, slot: usize) -> ItemStack;

    /// `setItem`。
    fn set(&mut self, slot: usize, stack: ItemStack);

    /// `isEmpty`。
    fn is_empty(&self) -> bool {
        (0..self.size()).all(|slot| self.get(slot).is_empty())
    }

    /// 这个物品栏是不是 `WorldlyContainer`，也就是有没有按面限定的槽。
    fn is_worldly(&self) -> bool {
        false
    }

    /// `getMaxStackSize()`。
    fn max_stack_size(&self) -> i32 {
        CONTAINER_STACK_LIMIT
    }

    /// `getMaxStackSize(ItemStack)`。
    fn max_stack_size_of(&self, stack: &ItemStack) -> i32 {
        self.max_stack_size().min(item_max(stack))
    }

    /// `canPlaceItem`。
    fn can_place_item(&self, _slot: usize, _stack: &ItemStack) -> bool {
        true
    }

    /// `canTakeItem`。
    fn can_take_item(&self, _slot: usize, _stack: &ItemStack) -> bool {
        true
    }

    /// `WorldlyContainer.getSlotsForFace`。不是 `WorldlyContainer` 时是 `None`。
    fn slots_for_face(&self, _direction: Direction) -> Option<&[usize]> {
        None
    }

    /// `WorldlyContainer.canPlaceItemThroughFace`。官方的方向可以为空，有些容器看它。
    fn can_place_item_through_face(
        &self,
        _slot: usize,
        _stack: &ItemStack,
        _direction: Option<Direction>,
    ) -> bool {
        true
    }

    /// `WorldlyContainer.canTakeItemThroughFace`。
    fn can_take_item_through_face(
        &self,
        _slot: usize,
        _stack: &ItemStack,
        _direction: Direction,
    ) -> bool {
        true
    }
}

/// 一个方块实体的槽位表，按官方默认的 `BaseContainerBlockEntity` 行为。
pub struct Slots<'a> {
    slots: &'a mut [ItemStack],
}

impl<'a> Slots<'a> {
    pub fn new(slots: &'a mut [ItemStack]) -> Self {
        Self { slots }
    }
}

impl Container for Slots<'_> {
    fn size(&self) -> usize {
        self.slots.len()
    }

    fn get(&self, slot: usize) -> ItemStack {
        self.slots[slot].clone()
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        self.slots[slot] = stack;
    }
}

/// `ShulkerBoxBlockEntity.SLOTS`：潜影盒的每个面都看整张表。
const SHULKER_SLOTS: [usize; 27] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26,
];

/// 潜影盒，官方 `ShulkerBoxBlockEntity`。
///
/// 它是 `WorldlyContainer`：每个面都能用全部 27 格，但潜影盒物品放不进去，盒子不能套盒子。
pub struct ShulkerSlots<'a> {
    slots: &'a mut [ItemStack],
}

impl<'a> ShulkerSlots<'a> {
    pub fn new(slots: &'a mut [ItemStack]) -> Self {
        Self { slots }
    }
}

impl Container for ShulkerSlots<'_> {
    fn size(&self) -> usize {
        self.slots.len()
    }

    fn get(&self, slot: usize) -> ItemStack {
        self.slots[slot].clone()
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        self.slots[slot] = stack;
    }

    fn is_worldly(&self) -> bool {
        true
    }

    fn slots_for_face(&self, _direction: Direction) -> Option<&[usize]> {
        Some(&SHULKER_SLOTS)
    }

    /// `canPlaceItemThroughFace`：`Block.byItem(itemStack.getItem()) instanceof ShulkerBoxBlock` 时不收。
    fn can_place_item_through_face(
        &self,
        _slot: usize,
        stack: &ItemStack,
        _direction: Option<Direction>,
    ) -> bool {
        let block = ITEMS
            .get(usize::try_from(stack.item).unwrap_or(usize::MAX))
            .and_then(|item| item.block);
        !block.is_some_and(|block| {
            blocks::BLOCKS[block as usize]
                .class
                .ancestors()
                .contains(&BlockAncestor::ShulkerBoxBlock)
        })
    }
}

/// 双箱，官方 `CompoundContainer`：先第一半，再第二半。
pub struct Combined<'a> {
    first: &'a mut [ItemStack],
    second: &'a mut [ItemStack],
}

impl<'a> Combined<'a> {
    pub fn new(first: &'a mut [ItemStack], second: &'a mut [ItemStack]) -> Self {
        Self { first, second }
    }
}

impl Container for Combined<'_> {
    fn size(&self) -> usize {
        self.first.len() + self.second.len()
    }

    fn get(&self, slot: usize) -> ItemStack {
        if slot < self.first.len() {
            self.first[slot].clone()
        } else {
            self.second[slot - self.first.len()].clone()
        }
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        if slot < self.first.len() {
            self.first[slot] = stack;
        } else {
            self.second[slot - self.first.len()] = stack;
        }
    }

    fn is_empty(&self) -> bool {
        self.first.iter().all(ItemStack::is_empty) && self.second.iter().all(ItemStack::is_empty)
    }
}

/// `AbstractContainerMenu.getRedstoneSignalFromContainer`。
///
/// 空的格子不贡献，每个非空格加上 `数量 / 这个物品的堆叠上限`，再除以槽位总数，
/// 最后按 `Mth.lerpDiscrete(total, 0, 15)` 映射到 0 到 15。
pub fn redstone_signal<'a>(slots: impl Iterator<Item = &'a ItemStack>, size: usize) -> i32 {
    if size == 0 {
        return 0;
    }
    let mut total = 0.0_f32;
    for stack in slots {
        if !stack.is_empty() {
            total += stack.count as f32 / item_max(stack) as f32;
        }
    }
    total /= size as f32;
    (total * 14.0).floor() as i32 + i32::from(total > 0.0)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;

    fn slots(count: usize) -> Vec<ItemStack> {
        vec![ItemStack::empty(); count]
    }

    #[test]
    fn stacks_merge_only_with_the_same_item_and_components() {
        let plain = ItemStack::new(items::STONE, 1);
        let mut componented = plain.clone();
        componented.added.push((1, vec![0x10]));
        assert!(same_item_same_components(&plain, &plain));
        assert!(!same_item_same_components(&plain, &componented));
        let mut removed = plain.clone();
        removed.removed.push(1);
        assert!(!same_item_same_components(&plain, &removed));
        assert!(same_item_same_components(
            &ItemStack::empty(),
            &ItemStack::empty()
        ));
    }

    #[test]
    fn removing_takes_from_the_front_of_the_stack() {
        let mut inventory = slots(2);
        inventory[0] = ItemStack::new(items::STONE, 5);
        let mut container = Slots::new(&mut inventory);
        let taken = remove_item(&mut container, 0, 2);
        assert_eq!(taken, ItemStack::new(items::STONE, 2));
        assert_eq!(container.get(0), ItemStack::new(items::STONE, 3));
        let rest = remove_item(&mut container, 0, 2);
        assert_eq!(rest, ItemStack::new(items::STONE, 2));
        assert_eq!(container.get(0), ItemStack::new(items::STONE, 1));
        assert!(remove_item(&mut container, 1, 1).is_empty());
    }

    #[test]
    fn a_double_chest_is_the_first_then_the_second_half() {
        let mut first = slots(27);
        let mut second = slots(27);
        first[3] = ItemStack::new(items::STONE, 1);
        second[2] = ItemStack::new(items::DIRT, 4);
        let mut combined = Combined::new(&mut first, &mut second);
        assert_eq!(combined.size(), 54);
        assert_eq!(combined.get(3), ItemStack::new(items::STONE, 1));
        assert_eq!(combined.get(29), ItemStack::new(items::DIRT, 4));
        combined.set(29, ItemStack::empty());
        assert!(combined.get(29).is_empty());
        assert!(!combined.is_empty());
        combined.set(3, ItemStack::empty());
        assert!(combined.is_empty());
    }

    #[test]
    fn the_signal_is_zero_for_empty_and_fifteen_for_full() {
        let empty = slots(27);
        assert_eq!(redstone_signal(empty.iter(), empty.len()), 0);

        // 一格装满的石头在 27 格里：1/27 * 14 = 0.51，向下取整再加一。
        let mut chest = slots(27);
        chest[0] = ItemStack::new(items::STONE, 64);
        assert_eq!(redstone_signal(chest.iter(), chest.len()), 1);

        let full = vec![ItemStack::new(items::STONE, 64); 27];
        assert_eq!(redstone_signal(full.iter(), full.len()), 15);

        // 漏斗只有 5 格，一摞满石头是 3。
        let mut hopper = slots(5);
        hopper[0] = ItemStack::new(items::STONE, 64);
        assert_eq!(redstone_signal(hopper.iter(), hopper.len()), 3);

        // 不可堆叠的物品一格就贡献满的 1/27。
        let mut single = slots(27);
        single[0] = ItemStack::new(items::DIAMOND_SWORD, 1);
        assert_eq!(redstone_signal(single.iter(), single.len()), 1);
    }
}
