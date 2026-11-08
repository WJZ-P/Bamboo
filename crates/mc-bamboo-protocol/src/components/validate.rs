//! 物品堆的组件合不合法，照官方 `ItemStack.validateStrict`。
//!
//! 官方解码不检查这些，只有 `ItemStackTemplate.create` 和 `ItemStack.validatedStreamCodec`
//! 会检查。想把客户端发来的组件转发给其他玩家时，先过一遍这里。
//!
//! 和官方一样看的是「物品的默认组件被补丁覆盖之后」的结果，默认组件来自
//! [`crate::components::defaults`]。收纳袋的总重量没算，见 README 的已知限制。

use mc_bamboo_data::registries::data_component_type as kinds;

use crate::{
    codec::DecodeError,
    components::{ComponentPatch, ComponentValue, PatchEntry, defaults, stack::ItemStackTemplate},
    item::ItemStack,
};

/// 官方 `ItemStack.validateStrict`：组件之间不矛盾，数量不超过上限。
///
/// 空物品堆没有组件可查，直接通过。
pub fn validate_stack(stack: &ItemStack) -> Result<(), DecodeError> {
    if stack.is_empty() {
        return Ok(());
    }
    let patch = ComponentPatch::from_opaque(stack)?;
    validate_components(stack.item, &patch)?;

    if stack.count > max_stack_size(stack.item, &patch) {
        return Err(DecodeError::InvalidItemCount(stack.count));
    }
    Ok(())
}

/// 官方 `ItemStack.validateComponents`。
pub fn validate_components(item: i32, patch: &ComponentPatch) -> Result<(), DecodeError> {
    // 官方：既能损坏又能堆叠是非法的。
    if defaults::resolve(item, patch, kinds::MAX_DAMAGE).is_some()
        && max_stack_size(item, patch) > 1
    {
        return Err(DecodeError::InvalidComponentValue(
            "item cannot be both damageable and stackable",
        ));
    }

    for (_, entry) in patch.entries() {
        let PatchEntry::Set(value) = entry else {
            continue;
        };
        match value {
            ComponentValue::Container(slots) => {
                for stack in slots.iter().flatten() {
                    validate_contained(stack)?;
                }
            }
            ComponentValue::BundleContents(stacks) | ComponentValue::ChargedProjectiles(stacks) => {
                for stack in stacks {
                    validate_contained(stack)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// 官方 `ItemStack.validateContainedItemSizes`，再往下递归。
fn validate_contained(stack: &ItemStackTemplate) -> Result<(), DecodeError> {
    if stack.count > stack.max_stack_size() {
        return Err(DecodeError::InvalidItemCount(stack.count));
    }
    validate_components(stack.item, &stack.components)
}

/// 官方 `ItemStack.getMaxStackSize`：补丁和默认组件合起来之后的 `max_stack_size`。
///
/// 官方 `Item.getDefaultMaxStackSize` 的兜底值也是 1。
pub fn max_stack_size(item: i32, patch: &ComponentPatch) -> i32 {
    match defaults::resolve(item, patch, kinds::MAX_STACK_SIZE) {
        Some(ComponentValue::MaxStackSize(size)) => *size,
        _ => 1,
    }
}
