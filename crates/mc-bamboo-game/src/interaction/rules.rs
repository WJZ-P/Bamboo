//! 物品的规则里 `mc_bamboo_blocks` 不管的部分：官方在进入方块逻辑之前和之后看的东西。

use mc_bamboo_data::{
    Item,
    items::{self, ITEMS},
};
use mc_bamboo_protocol::item::ItemStack;

/// 注册表 ID 对应的物品。
pub(crate) fn item(id: i32) -> Option<&'static Item> {
    ITEMS.get(usize::try_from(id).ok()?)
}

/// 手持这个物品能不能破坏方块，官方的 `ItemStack.canDestroyBlock`。`blocks-inventory.md` 5.2。
pub(super) fn can_destroy(held: &ItemStack) -> bool {
    item(held.item).is_none_or(|item| item.can_destroy_blocks_in_creative)
}

/// 官方的 `wasBlockPlacementAttempt`：这次右键算放置尝试，到高度上限时才有提示。
pub(super) fn is_placement_attempt(held: &ItemStack) -> bool {
    !held.is_empty()
        && (item(held.item).is_some_and(|item| item.block.is_some())
            || FLUID_BUCKETS.contains(&held.item))
}

/// 官方 `BucketItem` 里 `getContent()` 不是空的物品。`powder_snow_bucket` 是方块物品，不在这里。
const FLUID_BUCKETS: [i32; 8] = [
    items::WATER_BUCKET,
    items::LAVA_BUCKET,
    items::PUFFERFISH_BUCKET,
    items::SALMON_BUCKET,
    items::COD_BUCKET,
    items::TROPICAL_FISH_BUCKET,
    items::AXOLOTL_BUCKET,
    items::TADPOLE_BUCKET,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(item: i32) -> ItemStack {
        ItemStack::new(item, 1)
    }

    #[test]
    fn swords_cannot_destroy_in_creative_mode() {
        assert!(!can_destroy(&stack(items::DIAMOND_SWORD)));
        assert!(!can_destroy(&stack(items::DEBUG_STICK)));
        assert!(can_destroy(&stack(items::STONE)));
        assert!(can_destroy(&ItemStack::empty()));
    }

    #[test]
    fn placement_attempts_are_block_items_and_full_buckets() {
        assert!(is_placement_attempt(&stack(items::STONE)));
        assert!(is_placement_attempt(&stack(items::COMMAND_BLOCK)));
        assert!(is_placement_attempt(&stack(items::POWDER_SNOW_BUCKET)));
        assert!(is_placement_attempt(&stack(items::COD_BUCKET)));
        assert!(!is_placement_attempt(&stack(items::BUCKET)));
        assert!(!is_placement_attempt(&stack(items::SULFUR_CUBE_BUCKET)));
        assert!(!is_placement_attempt(&stack(items::STICK)));
        assert!(!is_placement_attempt(&ItemStack::empty()));
    }
}
