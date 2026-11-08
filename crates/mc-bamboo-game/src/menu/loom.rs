//! 织布机的图案列表和结果，对应 `LoomMenu.getSelectablePatterns` 和 `setupResultSlot`。
//!
//! 空的图案槽用 `minecraft:no_item_required` 的注册表顺序。图案物品用它的
//! `provides_banner_patterns`。结果在旗帜上再加一层，官方在 `slotsChanged` 里把层数卡在 6。
//!
//! Loom pattern lists and results, matching `LoomMenu.getSelectablePatterns` and `setupResultSlot`.
//!
//! An empty pattern slot uses `minecraft:no_item_required` in registry order. A pattern item uses
//! its `provides_banner_patterns`. The result appends one layer. `slotsChanged` caps the banner at 6.

use mc_bamboo_data::{
    ItemKind, item_tag,
    items::ITEMS,
    registries::data_component_type as kinds,
    tags::{self, item_tags},
};
use mc_bamboo_protocol::{
    components::{
        ComponentValue,
        cursor::{Holder, HolderSet},
        value::BannerLayer,
    },
    item::ItemStack,
};

use super::item_patch::{assign, resolved};

/// `LoomMenu.slotsChanged` 里的层数上限。数据包里没有这项。
///
/// The layer cap in `LoomMenu.slotsChanged`. It is not a data-pack value.
pub(super) const MAX_LAYERS: usize = 6;

const NO_ITEM_REQUIRED: &str = "minecraft:no_item_required";
const BANNER_PATTERN_REGISTRY: &str = "minecraft:banner_pattern";

/// 官方 `stack.getItem() instanceof BannerItem`。
///
/// Vanilla `stack.getItem() instanceof BannerItem`.
pub(super) fn is_banner(stack: &ItemStack) -> bool {
    item_kind(stack.item) == Some(ItemKind::Banner)
}

/// 官方 `isDyeItem`：在 `loom_dyes` 里，并且有 `dye` 组件。
///
/// Vanilla `isDyeItem`: in `loom_dyes`, and it has the `dye` component.
pub(super) fn is_dye(stack: &ItemStack) -> bool {
    tagged(stack.item, item_tags::LOOM_DYES.name) && resolved(stack, kinds::DYE).is_some()
}

/// 官方 `isPatternItem`：在 `loom_patterns` 里，并且有 `provides_banner_patterns`。
///
/// Vanilla `isPatternItem`: in `loom_patterns`, and it has `provides_banner_patterns`.
pub(super) fn is_pattern_item(stack: &ItemStack) -> bool {
    tagged(stack.item, item_tags::LOOM_PATTERNS.name)
        && resolved(stack, kinds::PROVIDES_BANNER_PATTERNS).is_some()
}

/// 官方 `getSelectablePatterns`。标签不存在时是空表。
///
/// Vanilla `getSelectablePatterns`. A missing tag is an empty list.
pub(super) fn selectable(pattern_item: &ItemStack) -> Vec<i32> {
    if pattern_item.is_empty() {
        return banner_tag(NO_ITEM_REQUIRED).to_vec();
    }
    match resolved(pattern_item, kinds::PROVIDES_BANNER_PATTERNS) {
        Some(ComponentValue::ProvidesBannerPatterns(HolderSet::Tag(name))) => {
            banner_tag(&name).to_vec()
        }
        Some(ComponentValue::ProvidesBannerPatterns(HolderSet::Ids(ids))) => ids,
        _ => Vec::new(),
    }
}

/// 旗帜上已经有几层。没有这个组件时是 0，和 `getOrDefault(EMPTY)` 一样。
///
/// How many layers the banner already has. A missing component is 0, like `getOrDefault(EMPTY)`.
pub(super) fn layer_count(stack: &ItemStack) -> usize {
    match resolved(stack, kinds::BANNER_PATTERNS) {
        Some(ComponentValue::BannerPatterns(layers)) => layers.len(),
        _ => 0,
    }
}

/// 官方 `setupResultSlot`：复制一份数量为 1 的旗帜，再加一层。染料没有颜色时结果是空的。
///
/// Vanilla `setupResultSlot`: copy the banner with count 1 and append one layer. No dye colour
/// leaves the result empty.
pub(super) fn with_layer(banner: &ItemStack, dye: &ItemStack, pattern: i32) -> ItemStack {
    if banner.is_empty() || dye.is_empty() {
        return ItemStack::empty();
    }
    let Some(ComponentValue::Dye(color)) = resolved(dye, kinds::DYE) else {
        return ItemStack::empty();
    };
    let mut result = banner.clone();
    result.count = 1;
    let mut layers = match resolved(&result, kinds::BANNER_PATTERNS) {
        Some(ComponentValue::BannerPatterns(layers)) => layers,
        _ => Vec::new(),
    };
    layers.push(BannerLayer {
        pattern: Holder::Id(pattern),
        color,
    });
    assign(
        &mut result,
        kinds::BANNER_PATTERNS,
        Some(ComponentValue::BannerPatterns(layers)),
    );
    result
}

fn item_kind(item: i32) -> Option<ItemKind> {
    usize::try_from(item)
        .ok()
        .and_then(|item| ITEMS.get(item))
        .map(|item| item.kind)
}

fn tagged(item: i32, name: &str) -> bool {
    item_tag(name).is_some_and(|ids| ids.contains(&item))
}

fn banner_tag(name: &str) -> &'static [i32] {
    tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == BANNER_PATTERN_REGISTRY)
        .and_then(|group| group.tags.iter().find(|tag| tag.name == name))
        .map(|tag| tag.ids)
        .unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;

    #[test]
    fn an_empty_pattern_slot_lists_no_item_required_in_tag_order() {
        let patterns = selectable(&ItemStack::empty());
        assert_eq!(patterns, banner_tag(NO_ITEM_REQUIRED));
        assert_eq!(patterns.first().copied(), Some(26));
        assert!(patterns.len() > 1);
    }

    #[test]
    fn white_dye_adds_one_layer_to_a_white_banner() {
        let banner = ItemStack::new(items::WHITE_BANNER, 4);
        let dye = ItemStack::new(items::WHITE_DYE, 1);
        let result = with_layer(&banner, &dye, 26);
        assert_eq!(result.item, items::WHITE_BANNER);
        assert_eq!(result.count, 1);
        assert_eq!(layer_count(&result), 1);
        assert!(is_banner(&banner));
        assert!(is_dye(&dye));
        assert!(!is_pattern_item(&dye));
    }

    #[test]
    fn a_stack_without_dye_colour_makes_no_result() {
        let banner = ItemStack::new(items::WHITE_BANNER, 1);
        let dirt = ItemStack::new(items::DIRT, 1);
        assert!(with_layer(&banner, &dirt, 26).is_empty());
        assert!(!is_dye(&dirt));
    }
}
