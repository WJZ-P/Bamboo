//! 收纳袋内容的类型化操作，照官方 `BundleContents` / `Mutable` 的字节码。
//!
//! 网络和存储里的 `bundle_contents` 都是一串 [`ItemStackTemplate`]：没有选中下标，
//! 也不带预先算好的重量。选中下标挂在 [`ItemStack::bundle_selected`] 上，随物品堆
//! 复制、挪动，不进网络字节。重量按官方的分数算法现算，嵌套收纳袋每层加 1/16，
//! 蜂箱（有 `bees`）占满一整袋，其余物品每个占 `1 / max_stack_size`。
//!
//! Vanilla `BundleContents` / `Mutable`: the codec only stores the item list. The
//! selected index lives on the stack, and the weight is recomputed as a fraction.

use mc_bamboo_data::{blocks, items, registries::data_component_type as kinds};

use crate::{
    components::{ComponentPatch, ComponentValue, PatchEntry, defaults, stack::ItemStackTemplate},
    item::ItemStack,
};

/// 收纳袋里再放一个收纳袋时额外占的份额，官方 `BUNDLE_IN_BUNDLE_WEIGHT`。
const BUNDLE_IN_BUNDLE_DENOMINATOR: i64 = 16;

/// 化简后的非负分数。分母恒为正，零用 `0/1` 表示。
///
/// Reduced non-negative fraction. The denominator is always positive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fraction {
    numerator: i64,
    denominator: i64,
}

impl Fraction {
    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    pub const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    /// 官方 `Fraction.getFraction(numerator, denominator)`：分母为 0 或溢出时算失败。
    pub fn new(numerator: i64, denominator: i64) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        let (numerator, denominator) = if denominator < 0 {
            (numerator.checked_neg()?, denominator.checked_neg()?)
        } else {
            (numerator, denominator)
        };
        let divisor = gcd(numerator.unsigned_abs(), denominator.unsigned_abs()) as i64;
        Some(Self {
            numerator: numerator.checked_div(divisor)?,
            denominator: denominator.checked_div(divisor)?,
        })
    }

    fn unit(denominator: i64) -> Option<Self> {
        Self::new(1, denominator)
    }

    pub fn plus(self, other: Self) -> Option<Self> {
        let denominator = lcm(self.denominator, other.denominator)?;
        let left = numerator_for(self, denominator)?;
        let right = numerator_for(other, denominator)?;
        Self::new(left.checked_add(right)?, denominator)
    }

    pub fn subtract(self, other: Self) -> Option<Self> {
        let denominator = lcm(self.denominator, other.denominator)?;
        let left = numerator_for(self, denominator)?;
        let right = numerator_for(other, denominator)?;
        Self::new(left.checked_sub(right)?, denominator)
    }

    pub fn multiply(self, other: Self) -> Option<Self> {
        Self::new(
            self.numerator.checked_mul(other.numerator)?,
            self.denominator.checked_mul(other.denominator)?,
        )
    }

    pub fn divide(self, other: Self) -> Option<Self> {
        if other.numerator == 0 {
            return None;
        }
        Self::new(
            self.numerator.checked_mul(other.denominator)?,
            self.denominator.checked_mul(other.numerator)?,
        )
    }

    pub fn compare(self, other: Self) -> Option<std::cmp::Ordering> {
        let denominator = lcm(self.denominator, other.denominator)?;
        let left = numerator_for(self, denominator)?;
        let right = numerator_for(other, denominator)?;
        Some(left.cmp(&right))
    }

    /// 官方 `Fraction.intValue()`：向零截断。负重量不会出现，这里直接做除法。
    pub fn int_value(self) -> i64 {
        self.numerator / self.denominator
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

fn lcm(a: i64, b: i64) -> Option<i64> {
    let divisor = gcd(a.unsigned_abs(), b.unsigned_abs()) as i64;
    a.checked_div(divisor)?.checked_mul(b)
}

fn numerator_for(value: Fraction, denominator: i64) -> Option<i64> {
    value
        .numerator
        .checked_mul(denominator.checked_div(value.denominator)?)
}

/// 官方 `ItemStack.canFitInsideContainerItems`：非空、不是潜影盒方块物品。
fn can_fit_inside(stack: &ItemStack) -> bool {
    if stack.is_empty() {
        return false;
    }
    let Some(item) = usize::try_from(stack.item)
        .ok()
        .and_then(|id| items::ITEMS.get(id))
    else {
        return false;
    };
    let Some(block) = item
        .block
        .and_then(|id| usize::try_from(id).ok())
        .and_then(|id| blocks::BLOCKS.get(id))
    else {
        return true;
    };
    // `BlockItem.canFitInsideContainerItems` 只拒绝潜影盒方块。
    // `BlockItem.canFitInsideContainerItems` refuses only shulker box blocks.
    !block.name.starts_with("minecraft:") || !is_shulker_box_name(block.name)
}

fn is_shulker_box_name(name: &str) -> bool {
    let Some(base) = name.strip_prefix("minecraft:") else {
        return false;
    };
    base == "shulker_box" || base.ends_with("_shulker_box")
}

/// `bundle_contents` 里的一件东西占多少：普通物品 `1 / max_stack_size`，
/// 嵌套收纳袋 `内层重量 + 1/16`，装了蜜蜂的蜂箱一整袋。
fn template_weight(template: &ItemStackTemplate) -> Option<Fraction> {
    let patch = &template.components;
    let nested = match defaults::resolve(template.item, patch, kinds::BUNDLE_CONTENTS) {
        Some(ComponentValue::BundleContents(items)) => Some(items),
        _ => None,
    };
    if let Some(items) = nested {
        return content_weight(items)?.plus(Fraction::unit(BUNDLE_IN_BUNDLE_DENOMINATOR)?);
    }
    if matches!(
        defaults::resolve(template.item, patch, kinds::BEES),
        Some(ComponentValue::Bees(bees)) if !bees.is_empty()
    ) {
        return Some(Fraction::ONE);
    }
    Fraction::unit(i64::from(template.max_stack_size().max(1)))
}

/// 官方 `computeContentWeight`。算术溢出时和官方 `DataResult.error` 一样算失败。
fn content_weight(items: &[ItemStackTemplate]) -> Option<Fraction> {
    let mut total = Fraction::ZERO;
    for item in items {
        let weight = template_weight(item)?;
        let count = Fraction::new(i64::from(item.count.max(0)), 1)?;
        total = total.plus(weight.multiply(count)?)?;
    }
    Some(total)
}

/// 一个物品堆里的 `bundle_contents`。组件解不开或没有这个组件时返回 `None`。
///
/// Returns `None` when the component is absent or cannot be decoded.
pub fn contents(stack: &ItemStack) -> Option<Vec<ItemStackTemplate>> {
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    match patch.get(kinds::BUNDLE_CONTENTS) {
        Some(ComponentValue::BundleContents(items)) => Some(items.clone()),
        _ => None,
    }
}

/// 写下 `bundle_contents`。内容为空时按官方把组件设成空列表，而不是从补丁里删掉。
fn set_contents(stack: &mut ItemStack, items: Vec<ItemStackTemplate>) {
    let Ok(mut patch) = ComponentPatch::from_opaque(stack) else {
        return;
    };
    patch.put(
        kinds::BUNDLE_CONTENTS,
        PatchEntry::Set(ComponentValue::BundleContents(items)),
    );
    let (added, removed) = patch.to_opaque();
    patch.release();
    stack.added = added;
    stack.removed = removed;
}

/// 官方 `BundleContents.asMutable`：`weight()` 算不出来时给一个空的可变容器，
/// 选中下标也一起丢掉。
struct Mutable {
    items: Vec<ItemStack>,
    weight: Fraction,
    selected: Option<u32>,
    needs_flattening: bool,
}

impl Mutable {
    fn of(stack: &ItemStack) -> Option<Self> {
        let templates = contents(stack)?;
        let weight = content_weight(&templates);
        let mut mutable = Self {
            items: Vec::with_capacity(templates.len()),
            weight: weight.unwrap_or(Fraction::ZERO),
            selected: if weight.is_some() {
                stack.bundle_selected
            } else {
                None
            },
            needs_flattening: false,
        };
        if weight.is_some() {
            for template in templates {
                mutable.items.push(template_stack(&template));
            }
        }
        Some(mutable)
    }

    /// 官方 `getMaxAmountToAdd`：`floor((1 - weight) / unit)`，负数夹到 0。
    fn max_amount_to_add(&self, unit: Fraction) -> i32 {
        let Some(room) = Fraction::ONE.subtract(self.weight) else {
            return 0;
        };
        let Some(amount) = room.divide(unit) else {
            return 0;
        };
        amount.int_value().clamp(0, i64::from(i32::MAX)) as i32
    }

    /// 官方 `tryInsert`：把 `input` 里能放下的部分搬进袋里，返回搬了几件。
    /// 同一种物品的那一堆移到列表最前面（`addFirst`）。
    fn try_insert(&mut self, input: &mut ItemStack) -> i32 {
        if !can_fit_inside(input) {
            return 0;
        }
        let Some(unit) = stack_weight(input) else {
            return 0;
        };
        let amount = input.count.min(self.max_amount_to_add(unit));
        if amount <= 0 {
            return 0;
        }
        let Some(stacked) = Fraction::new(i64::from(amount), 1) else {
            return 0;
        };
        let Some(added) = unit.multiply(stacked) else {
            return 0;
        };
        let Some(weight) = self.weight.plus(added) else {
            return 0;
        };
        self.weight = weight;
        if let Some(index) = self.find_stack_index(input) {
            let mut existing = self.items.remove(index);
            existing.count += amount;
            self.items.insert(0, existing);
            input.count -= amount;
            if input.count <= 0 {
                *input = ItemStack::empty();
            }
        } else {
            let split = split(input, amount);
            self.items.insert(0, split);
        }
        amount
    }

    /// 官方 `tryTransfer`：先从槽里安全拿走一部分，再试着装进去。
    fn try_transfer(&mut self, slot: &mut ItemStack, max_take: i32) -> i32 {
        let Some(unit) = stack_weight(slot) else {
            return 0;
        };
        let amount = self.max_amount_to_add(unit).min(max_take);
        if !can_fit_inside(slot) {
            return 0;
        }
        let mut taken = safe_take(slot, amount);
        self.try_insert(&mut taken)
    }

    fn find_stack_index(&self, stack: &ItemStack) -> Option<usize> {
        if !stack.is_stackable() {
            return None;
        }
        self.items
            .iter()
            .position(|item| item.is_same_item_same_components(stack))
    }

    /// 官方 `removeOne`：取走选中下标的那一整堆（下标无效时取第 0 堆），
    /// 然后清掉选中状态。
    fn remove_one(&mut self) -> Option<ItemStack> {
        if self.items.is_empty() {
            return None;
        }
        let index = self
            .selected
            .and_then(|index| usize::try_from(index).ok())
            .filter(|&index| index < self.items.len())
            .unwrap_or(0);
        let taken = self.items.remove(index);
        self.weight = self
            .weight
            .subtract(stacked_weight(&taken).unwrap_or(Fraction::ZERO))
            .unwrap_or(self.weight);
        self.selected = None;
        Some(taken)
    }

    /// 官方 `toggleSelectedItem`：再点同一格或下标越界就取消选中。
    fn toggle_selected(&mut self, index: i32) {
        self.selected = if self.selected == usize::try_from(index).ok().map(|i| i as u32)
            || index < 0
            || usize::try_from(index)
                .ok()
                .is_none_or(|i| i >= self.items.len())
        {
            None
        } else {
            Some(index as u32)
        };
    }

    /// 官方 `toImmutable`：合并同种物品、去掉空格，写回补丁。
    fn write_back(&mut self, stack: &mut ItemStack) {
        self.merge_identical_stacks();
        let items = self
            .items
            .iter()
            .filter(|item| !item.is_empty())
            .map(template_of)
            .collect();
        set_contents(stack, items);
        stack.bundle_selected = self.selected;
    }

    /// `mergeIdenticalStacks`：`setItem` 改过格子之后才需要跑。
    fn merge_identical_stacks(&mut self) {
        if !self.needs_flattening {
            return;
        }
        let mut index = 0usize;
        while index < self.items.len() {
            if self.items[index].is_empty() {
                self.items.remove(index);
                continue;
            }
            let mut found = None;
            for other in index + 1..self.items.len() {
                if self.items[other].is_stackable()
                    && self.items[other].is_same_item_same_components(&self.items[index])
                {
                    found = Some(other);
                    break;
                }
            }
            if let Some(other) = found {
                let count = self.items[index].count + self.items[other].count;
                self.items[other].count = count;
                self.items.remove(index);
            } else {
                index += 1;
            }
        }
        self.needs_flattening = false;
    }
}

fn template_stack(template: &ItemStackTemplate) -> ItemStack {
    let (added, removed) = template.components.to_opaque();
    ItemStack {
        item: template.item,
        count: template.count,
        added,
        removed,
        bundle_selected: None,
    }
}

fn template_of(stack: &ItemStack) -> ItemStackTemplate {
    ItemStackTemplate {
        item: stack.item,
        count: stack.count.max(1),
        components: ComponentPatch::from_opaque(stack).unwrap_or_default(),
    }
}

/// 官方 `BundleContents.getWeight` 对一堆物品的份额（不含数量）。
fn stack_weight(stack: &ItemStack) -> Option<Fraction> {
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    if let Some(ComponentValue::BundleContents(items)) = patch.get(kinds::BUNDLE_CONTENTS) {
        return content_weight(items)?.plus(Fraction::unit(BUNDLE_IN_BUNDLE_DENOMINATOR)?);
    }
    if matches!(
        defaults::resolve(stack.item, &patch, kinds::BEES),
        Some(ComponentValue::Bees(bees)) if !bees.is_empty()
    ) {
        return Some(Fraction::ONE);
    }
    Fraction::unit(i64::from(stack.max_stack_size().max(1)))
}

fn stacked_weight(stack: &ItemStack) -> Option<Fraction> {
    let count = Fraction::new(i64::from(stack.count.max(0)), 1)?;
    stack_weight(stack)?.multiply(count)
}

fn split(stack: &mut ItemStack, amount: i32) -> ItemStack {
    let taken = amount.min(stack.count).max(0);
    let mut piece = stack.clone();
    piece.count = taken;
    stack.count -= taken;
    if stack.count <= 0 {
        *stack = ItemStack::empty();
    }
    piece
}

fn safe_take(slot: &mut ItemStack, amount: i32) -> ItemStack {
    split(slot, amount.max(0).min(slot.count))
}

/// 官方 `BundleItem.toggleSelectedItem`：切换选中格，不改网络上的补丁。
pub fn toggle_selected(stack: &mut ItemStack, index: i32) {
    let Some(mut mutable) = Mutable::of(stack) else {
        return;
    };
    mutable.toggle_selected(index);
    mutable.write_back(stack);
}

/// 官方 `BundleContents.Mutable.removeOne`，成功时 `bundle_selected` 复位。
pub fn remove_one(stack: &mut ItemStack) -> Option<ItemStack> {
    let mut mutable = Mutable::of(stack)?;
    let taken = mutable.remove_one();
    mutable.write_back(stack);
    taken
}

/// 官方 `BundleContents.Mutable.tryInsert`，返回装进去的数量。
pub fn try_insert(bundle: &mut ItemStack, input: &mut ItemStack) -> i32 {
    let Some(mut mutable) = Mutable::of(bundle) else {
        return 0;
    };
    let inserted = mutable.try_insert(input);
    mutable.write_back(bundle);
    inserted
}

/// 官方 `BundleContents.Mutable.tryTransfer`：从 `slot` 里最多拿它现在的数量。
pub fn try_transfer(bundle: &mut ItemStack, slot: &mut ItemStack) -> i32 {
    let Some(mut mutable) = Mutable::of(bundle) else {
        return 0;
    };
    let max_take = slot.count;
    let inserted = mutable.try_transfer(slot, max_take);
    mutable.write_back(bundle);
    inserted
}

/// 内容物的副本，官方 `BundleContents.itemCopies`。
pub fn item_copies(stack: &ItemStack) -> Vec<ItemStack> {
    contents(stack)
        .unwrap_or_default()
        .iter()
        .map(template_stack)
        .collect()
}

/// 收纳袋物品本身（`minecraft:bundles` 标签里的 17 种）。
pub fn is_bundle_item(item: i32) -> bool {
    mc_bamboo_data::item_tag("minecraft:bundles").is_some_and(|ids| ids.contains(&item))
}

/// 把物品堆的 `bundle_contents` 替换成空列表，官方 `onDestroyed` 的第一半。
pub fn clear_contents(stack: &mut ItemStack) {
    set_contents(stack, Vec::new());
    stack.bundle_selected = None;
}

/// 官方 `BundleItem.onDestroyed`：把内容物一件一件原样掉在原地。
/// 调用方负责把这些物品生成实体。
pub fn destroyed_contents(stack: &mut ItemStack) -> Vec<ItemStack> {
    let items = item_copies(stack);
    clear_contents(stack);
    items
}

/// 官方 `BundleContents.getWeight` 对一件物品的份额，供测试和容量显示用。
pub fn weight_of(stack: &ItemStack) -> Option<Fraction> {
    stack_weight(stack)
}

/// 官方 `BundleContents.weight()`：整袋重量。
pub fn weight(stack: &ItemStack) -> Option<Fraction> {
    let items = contents(stack)?;
    content_weight(&items)
}

/// 容器物品被毁时把内容组件清空。返回是否真的改过。
pub fn clear_contents_if_container(stack: &mut ItemStack) -> bool {
    let Ok(mut patch) = ComponentPatch::from_opaque(stack) else {
        return false;
    };
    let kind = if patch.get(kinds::BUNDLE_CONTENTS).is_some() {
        kinds::BUNDLE_CONTENTS
    } else if patch.get(kinds::CONTAINER).is_some() {
        kinds::CONTAINER
    } else {
        return false;
    };
    let empty = if kind == kinds::BUNDLE_CONTENTS {
        ComponentValue::BundleContents(Vec::new())
    } else {
        ComponentValue::Container(Vec::new())
    };
    patch.put(kind, PatchEntry::Set(empty));
    let (added, removed) = patch.to_opaque();
    patch.release();
    stack.added = added;
    stack.removed = removed;
    stack.bundle_selected = None;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::items;

    fn bundle(items: &[(i32, i32)]) -> ItemStack {
        let mut stack = ItemStack::new(items::BUNDLE, 1);
        let templates = items
            .iter()
            .map(|&(item, count)| ItemStackTemplate {
                item,
                count,
                components: ComponentPatch::new(),
            })
            .collect();
        set_contents(&mut stack, templates);
        stack
    }

    #[test]
    fn a_stack_of_sixty_four_fills_the_bundle() {
        let mut bag = bundle(&[]);
        let mut stone = ItemStack::new(items::STONE, 64);
        assert_eq!(try_insert(&mut bag, &mut stone), 64);
        assert!(stone.is_empty());
        assert_eq!(weight(&bag), Some(Fraction::ONE));
        assert_eq!(contents(&bag).unwrap()[0].count, 64);
    }

    #[test]
    fn a_full_bundle_refuses_more() {
        let mut bag = bundle(&[(items::STONE, 64)]);
        let mut dirt = ItemStack::new(items::DIRT, 1);
        assert_eq!(try_insert(&mut bag, &mut dirt), 0);
        assert_eq!(dirt.count, 1);
        assert_eq!(contents(&bag).unwrap().len(), 1);
    }

    #[test]
    fn identical_stacks_move_to_the_front() {
        let mut bag = bundle(&[(items::DIRT, 3)]);
        let mut stone = ItemStack::new(items::STONE, 2);
        assert_eq!(try_insert(&mut bag, &mut stone), 2);
        let items = contents(&bag).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].item, items::STONE);
        assert_eq!(items[1].item, items::DIRT);
        let mut more = ItemStack::new(items::DIRT, 1);
        assert_eq!(try_insert(&mut bag, &mut more), 1);
        let items = contents(&bag).unwrap();
        assert_eq!(items[0].item, items::DIRT);
        assert_eq!(items[0].count, 4);
        assert_eq!(items[1].item, items::STONE);
    }

    #[test]
    fn a_nested_bundle_adds_one_sixteenth() {
        let inner = bundle(&[(items::STONE, 32)]);
        let template = template_of(&inner);
        assert_eq!(
            template_weight(&template),
            Some(Fraction::new(9, 16).unwrap())
        );
        let mut outer = bundle(&[]);
        let mut input = inner;
        assert_eq!(try_insert(&mut outer, &mut input), 1);
        assert_eq!(weight(&outer), Some(Fraction::new(9, 16).unwrap()));
        // 剩下 7/16，普通方块最多再放 44 个（44/64 = 11/16 放不下）。
        let mut stone = ItemStack::new(items::STONE, 64);
        assert_eq!(try_insert(&mut outer, &mut stone), 28);
        assert_eq!(stone.count, 36);
    }

    #[test]
    fn selecting_a_slot_changes_which_stack_comes_out() {
        let mut bag = bundle(&[(items::STONE, 3), (items::DIRT, 2), (items::APPLE, 1)]);
        toggle_selected(&mut bag, 1);
        assert_eq!(bag.bundle_selected, Some(1));
        let taken = remove_one(&mut bag).unwrap();
        assert_eq!(taken.item, items::DIRT);
        assert_eq!(taken.count, 2);
        assert_eq!(bag.bundle_selected, None);
        let items = contents(&bag).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].item, items::STONE);
    }

    #[test]
    fn selecting_out_of_range_clears_the_selection() {
        let mut bag = bundle(&[(items::STONE, 1)]);
        toggle_selected(&mut bag, 1);
        assert_eq!(bag.bundle_selected, None);
        toggle_selected(&mut bag, 0);
        assert_eq!(bag.bundle_selected, Some(0));
        toggle_selected(&mut bag, 0);
        assert_eq!(bag.bundle_selected, None);
    }

    #[test]
    fn destroyed_contents_are_dropped_and_the_bag_is_emptied() {
        let mut bag = bundle(&[(items::STONE, 3), (items::DIRT, 2)]);
        let dropped = destroyed_contents(&mut bag);
        assert_eq!(dropped.len(), 2);
        assert_eq!(dropped[0].item, items::STONE);
        assert!(contents(&bag).unwrap().is_empty());
    }

    #[test]
    fn a_shulker_box_cannot_go_inside() {
        let mut bag = bundle(&[]);
        let mut shulker = ItemStack::new(items::SHULKER_BOX, 1);
        assert_eq!(try_insert(&mut bag, &mut shulker), 0);
        assert_eq!(shulker.count, 1);
    }

    #[test]
    fn every_bundle_item_is_in_the_tag() {
        assert!(is_bundle_item(items::BUNDLE));
        assert!(is_bundle_item(items::BLACK_BUNDLE));
        assert!(!is_bundle_item(items::STONE));
    }
}
