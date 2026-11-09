//! 制图台的格子限制和结果，对应 `CartographyTableMenu`。
//!
//! Bamboo 没有 `MapItemSavedData`；真实世界里按官方「找不到地图数据」的分支处理，
//! 也就是 `setupResultSlot` 里直接 return，不碰结果格。菜单 oracle 的假世界会补上
//! 比例和锁定状态，让三支结果都能和官方逐槽比较。

use mc_bamboo_data::{item_tag, items, registries::data_component_type as kinds};
use mc_bamboo_protocol::{components::ComponentValue, item::ItemStack};

use super::item_patch::{assign, resolved};

/// `MapPostProcessing` 枚举：LOCK=0、SCALE=1。
pub(super) const LOCK: i32 = 0;
pub(super) const SCALE: i32 = 1;
/// `MapItemSavedData.MAX_SCALE`。
pub(super) const MAX_SCALE: i32 = 4;

/// `CartographyTableMenu$3.mayPlace`：地图槽只收带 `map_id` 的物品。
pub(super) fn is_map(stack: &ItemStack) -> bool {
    resolved(stack, kinds::MAP_ID).is_some()
}

/// `CartographyTableMenu$4.mayPlace`。
pub(super) fn is_additional(stack: &ItemStack) -> bool {
    matches!(stack.item, items::PAPER | items::MAP | items::GLASS_PANE)
}

/// `quickMoveStack` 里判断背包物品能不能进地图槽。
pub(super) fn has_map_id(stack: &ItemStack) -> bool {
    resolved(stack, kinds::MAP_ID).is_some()
}

/// `ItemTags.EXTENDABLE_MAPS`。
pub(super) fn extendable(stack: &ItemStack) -> bool {
    item_tag("minecraft:extendable_maps").is_some_and(|ids| ids.contains(&stack.item))
}

/// `CartographyTableMenu.lambda$setupResultSlot$0` 里能靠物品组件处理的三支。
///
/// 真实世界没有地图存档，按官方「找不到地图数据」的分支处理：结果格保持原样，
/// 重放里由假世界提供 `scale`/`locked`，能按官方算出复制、放大和锁定。
pub(super) fn result(
    map: &ItemStack,
    additional: &ItemStack,
    old: &ItemStack,
    scale: i32,
    locked: bool,
) -> ItemStack {
    if additional.item == items::PAPER && extendable(map) && !locked && scale < MAX_SCALE {
        let mut result = copy_with_count(map, 1);
        set_post_processing(&mut result, SCALE);
        return if result.matches(old) {
            old.clone()
        } else {
            result
        };
    } else if additional.item == items::GLASS_PANE && !locked {
        let mut result = copy_with_count(map, 1);
        set_post_processing(&mut result, LOCK);
        return if result.matches(old) {
            old.clone()
        } else {
            result
        };
    } else if additional.item == items::MAP {
        let result = copy_with_count(map, 2);
        return if result.matches(old) {
            old.clone()
        } else {
            result
        };
    }
    ItemStack::empty()
}

/// 复制一份数量为 `count` 的地图，保留组件补丁。
pub(super) fn copy_with_count(stack: &ItemStack, count: i32) -> ItemStack {
    let mut copy = stack.clone();
    copy.count = count;
    copy
}

/// 官方 `setupResultSlot` 里给结果写 `MAP_POST_PROCESSING` 的那一步。
pub(super) fn set_post_processing(stack: &mut ItemStack, kind: i32) {
    assign(
        stack,
        kinds::MAP_POST_PROCESSING,
        Some(ComponentValue::MapPostProcessing(kind)),
    );
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::components::ComponentValue;

    use super::*;

    fn stack(item: i32, count: i32) -> ItemStack {
        ItemStack::new(item, count)
    }

    #[test]
    fn only_the_three_additional_items_are_accepted() {
        assert!(is_additional(&stack(items::PAPER, 1)));
        assert!(is_additional(&stack(items::MAP, 1)));
        assert!(is_additional(&stack(items::GLASS_PANE, 1)));
        assert!(!is_additional(&stack(items::STICK, 1)));
        assert!(!is_map(&stack(items::FILLED_MAP, 1)));
    }

    #[test]
    fn a_map_id_component_makes_the_map_slot_accept_it() {
        let mut map = stack(items::FILLED_MAP, 1);
        assign(&mut map, kinds::MAP_ID, Some(ComponentValue::MapId(7)));
        assert!(is_map(&map));
        assert!(has_map_id(&map));
        assert!(extendable(&map));
    }

    #[test]
    fn post_processing_values_match_the_official_enum() {
        assert_eq!(LOCK, 0);
        assert_eq!(SCALE, 1);
        let mut map = stack(items::FILLED_MAP, 1);
        set_post_processing(&mut map, SCALE);
        assert_eq!(
            resolved(&map, kinds::MAP_POST_PROCESSING),
            Some(ComponentValue::MapPostProcessing(SCALE))
        );
    }
}
