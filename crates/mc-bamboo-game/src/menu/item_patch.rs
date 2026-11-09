//! 菜单里改物品组件的小函数，对应 `PatchedDataComponentMap.set`。
//!
//! 值等于这个物品的默认组件时条目消失；移除一个本来就没有默认值的组件时也不留条目。
//!
//! Small helpers for editing item components in a menu, matching `PatchedDataComponentMap.set`.
//!
//! A value equal to the item's default drops the patch entry. Removing a component that has no
//! default leaves no entry either.

use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, defaults},
    item::ItemStack,
};

/// 一个物品栈上的组件补丁。解不开时按空补丁，跟配方那边一样。
///
/// The stack's component patch. An undecodable patch is treated as empty, same as recipes.
pub(super) fn patch_of(stack: &ItemStack) -> ComponentPatch {
    ComponentPatch::from_opaque(stack).unwrap_or_default()
}

/// 把补丁写回物品栈。
///
/// Write a patch back onto a stack.
pub(super) fn write_patch(stack: &mut ItemStack, patch: &ComponentPatch) {
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
}

/// 官方 `PatchedDataComponentMap.set`。`None` 是移除。
///
/// Vanilla `PatchedDataComponentMap.set`. `None` removes the component.
pub(super) fn assign(stack: &mut ItemStack, kind: i32, value: Option<ComponentValue>) {
    if stack.is_empty() {
        return;
    }
    let mut patch = patch_of(stack);
    let default = defaults::of_item(stack.item).get(kind);
    match value {
        Some(value) => {
            if default == Some(&value) {
                patch.remove(kind);
            } else {
                patch.put(kind, PatchEntry::Set(value));
            }
        }
        None => {
            if default.is_some() {
                patch.put(kind, PatchEntry::Removed);
            } else {
                patch.remove(kind);
            }
        }
    }
    write_patch(stack, &patch);
}

/// 官方 `ItemStack.get`：补丁盖过默认组件。
///
/// Vanilla `ItemStack.get`: the patch overrides the default component.
pub(super) fn resolved(stack: &ItemStack, kind: i32) -> Option<ComponentValue> {
    defaults::resolve(stack.item, &patch_of(stack), kind).cloned()
}
