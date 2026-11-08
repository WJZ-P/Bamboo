//! 物品堆的组件合不合法，照官方 `ItemStack.validateStrict`。
//!
//! 官方解码不检查这些，只有 `ItemStackTemplate.create` 和 `ItemStack.validatedStreamCodec`
//! 会检查。想把客户端发来的组件转发给其他玩家时，先过一遍这里。

use mc_bamboo_data::items::ITEMS;

use crate::{
    codec::DecodeError,
    components::{ComponentPatch, ComponentValue, PatchEntry, stack::ItemStackTemplate},
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
    validate_components(&patch)?;

    let max = max_stack_size(stack.item, &patch);
    if stack.count > max {
        return Err(DecodeError::InvalidItemCount(stack.count));
    }
    Ok(())
}

/// 官方 `ItemStack.validateComponents`。
pub fn validate_components(patch: &ComponentPatch) -> Result<(), DecodeError> {
    // 官方：既能损坏又能堆叠是非法的。官方看的是物品默认组件和补丁合起来的结果，
    // 这里只有补丁里的 `max_damage` 和物品的默认堆叠上限，见模块文档里的限制。
    if patch
        .get(mc_bamboo_data::registries::data_component_type::MAX_DAMAGE)
        .is_some()
        && patch.max_stack_size().unwrap_or(1) > 1
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
    validate_components(&stack.components)
}

/// 一个物品堆的堆叠上限：补丁里的 `max_stack_size`，否则物品自己的默认值。
fn max_stack_size(item: i32, patch: &ComponentPatch) -> i32 {
    patch.max_stack_size().unwrap_or_else(|| {
        usize::try_from(item)
            .ok()
            .and_then(|id| ITEMS.get(id))
            .map_or(1, |item| item.max_stack_size)
    })
}
