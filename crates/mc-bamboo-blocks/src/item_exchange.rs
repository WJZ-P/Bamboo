//! 炼药锅交还给玩家的那件物品，对应官方 `ItemUtils.createFilledResult` 和 `ItemStack.remove`。
//!
//! 方块逻辑不碰玩家的物品栏。它只记下要换上的物品，游戏层再按官方的规则放进去。
//!
//! What a cauldron hands back, matching `ItemUtils.createFilledResult` and `ItemStack.remove`.
//!
//! Block behaviour does not touch the player's inventory. It records the replacement, and the
//! game layer inserts it the way vanilla does.

use mc_bamboo_data::{items, recipes, registries::data_component_type as kinds};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, defaults},
    item::ItemStack,
};

/// 一次右键要改手里的物品。
///
/// How one click changes the stack in the player's hand.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemExchange {
    /// `ItemUtils.createFilledResult`。`limit_creative` 为真时，创造模式的物品栏里已经有
    /// 同样的结果就不再加一份。
    ///
    /// `ItemUtils.createFilledResult`. When `limit_creative` is set, creative mode adds a copy
    /// only if the inventory does not already contain that result.
    Fill {
        replacement: ItemStack,
        limit_creative: bool,
    },
    /// `itemInHand.remove(DataComponents.DYED_COLOR)`：整堆去掉颜色，数量不变。
    ///
    /// `itemInHand.remove(DataComponents.DYED_COLOR)`: the whole stack loses its colour and
    /// keeps its count.
    RemoveDyedColor,
    /// `Inventory.add`，放不下的部分由游戏层按 `Player.drop` 丢掉。
    ///
    /// `Inventory.add`. The game layer `Player.drop`s whatever does not fit.
    AddOrDrop(ItemStack),
    /// `BeehiveBlock` 用玻璃瓶：哪怕创造模式也先 `shrink(1)`，空了就换上蜜瓶，否则 `add`。
    ///
    /// A glass bottle on a beehive: `shrink(1)` even in creative. An empty hand becomes a honey
    /// bottle; otherwise `Inventory.add` takes it.
    BottleHoney,
}

/// 官方 `PotionContents.createItemStack(Items.POTION, Potions.WATER)`。
///
/// `PotionContents.createItemStack(Items.POTION, Potions.WATER)`.
pub(crate) fn water_potion_stack() -> ItemStack {
    let mut stack = ItemStack::new(items::POTION, 1);
    if let Some(id) = water_potion_id() {
        set_component(
            &mut stack,
            kinds::POTION_CONTENTS,
            ComponentValue::PotionContents {
                potion: Some(id),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            },
        );
    }
    stack
}

/// `minecraft:potion` 注册表里 `minecraft:water` 的 ID。表由生成器导出。
///
/// Registry id of `minecraft:water` in `minecraft:potion`. The table is generated.
pub(crate) fn water_potion_id() -> Option<i32> {
    recipes::POTIONS
        .iter()
        .position(|name| *name == "minecraft:water")
        .and_then(|id| i32::try_from(id).ok())
}

/// 官方 `ItemStack.remove(DataComponents.DYED_COLOR)`。
///
/// `ItemStack.remove(DataComponents.DYED_COLOR)`.
pub fn wash_dyed_color(stack: &mut ItemStack) {
    remove_component(stack, kinds::DYED_COLOR);
}

/// 官方 `ItemStack.transmuteCopy`：把这一份补丁套到新物品的默认组件上。
///
/// `ItemStack.transmuteCopy`: apply this stack's patch onto the new item's defaults.
pub(crate) fn transmute_copy(stack: &ItemStack, item: i32, count: i32) -> ItemStack {
    if stack.is_empty() {
        return ItemStack::empty();
    }
    let patch = stack.patch();
    let mut result = ItemStack::new(item, count);
    for (kind, entry) in patch.entries() {
        match entry {
            PatchEntry::Set(value) => set_component(&mut result, *kind, value.clone()),
            PatchEntry::Removed => remove_component(&mut result, *kind),
        }
    }
    result
}

/// 官方 `PatchedDataComponentMap.set`：新值和默认组件相同就从补丁里拿掉。
///
/// `PatchedDataComponentMap.set`: a value equal to the default is dropped from the patch.
pub(crate) fn set_component(stack: &mut ItemStack, kind: i32, value: ComponentValue) {
    if stack.is_empty() {
        return;
    }
    let mut patch = stack.patch();
    if defaults::of_item(stack.item).get(kind) == Some(&value) {
        patch = without(&patch, kind);
    } else {
        patch.put(kind, PatchEntry::Set(value));
    }
    write_patch(stack, &patch);
}

/// 官方 `PatchedDataComponentMap.remove`：默认组件里有它才记下「移除」。
///
/// `PatchedDataComponentMap.remove`: record `Removed` only when the prototype has the component.
pub(crate) fn remove_component(stack: &mut ItemStack, kind: i32) {
    if stack.is_empty() {
        return;
    }
    let mut patch = stack.patch();
    if defaults::of_item(stack.item).get(kind).is_some() {
        patch.put(kind, PatchEntry::Removed);
    } else {
        patch = without(&patch, kind);
    }
    write_patch(stack, &patch);
}

/// 补丁里不再提这个组件。
///
/// Drop one component from the patch entirely.
fn without(patch: &ComponentPatch, kind: i32) -> ComponentPatch {
    let mut kept = ComponentPatch::new();
    for (id, entry) in patch.entries() {
        if *id != kind {
            kept.put(*id, entry.clone());
        }
    }
    kept
}

/// 把类型化的补丁写回物品堆的字节。
///
/// Store a typed patch back into the stack's raw bytes.
fn write_patch(stack: &mut ItemStack, patch: &ComponentPatch) {
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
}
