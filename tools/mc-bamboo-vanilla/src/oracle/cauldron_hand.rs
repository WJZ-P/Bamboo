//! 点到炼药锅时，给手里的物品挂上官方 `CauldronInteractions` 要看的组件。
//!
//! 和 `BlockOracle.attachCauldronComponents` 用同一套规则：只在炼药锅上挂，而且旗帜的图案只在
//! 不蹲着的水炼药锅上挂。别的方块不挂，否则水瓶会把泥土变成泥，带图案的旗帜会写进方块实体。
//!
//! Attach the components `CauldronInteractions` inspects, matching
//! `BlockOracle.attachCauldronComponents`. Only a cauldron gets them, and a banner pattern is
//! attached only on a water cauldron the player is not sneaking against. Other blocks stay plain:
//! a water potion turns dirt into mud, and a banner pattern is stored on the block entity.

use mc_bamboo_blocks::{BlockState, state::item_is_tagged};
use mc_bamboo_data::{blocks, items, registries::data_component_type as kinds, tags::item_tags};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, cursor::Holder, value::BannerLayer},
    item::ItemStack,
};

/// 和 Java 里的 `new DyedItemColor(0xFF3366)` 是同一个颜色。
///
/// The same rgb as Java `new DyedItemColor(0xFF3366)`.
const DYE_RGB: i32 = 0x00FF_3366;
/// `DyeColor.WHITE` 是这个枚举声明的第一个常量，网络 ID 是 0。
///
/// `DyeColor.WHITE` is the first declared constant, so its network id is 0.
const WHITE_DYE: i32 = 0;

/// 这次点击要放进手里的物品堆。不需要组件时是 `None`，重放仍只传物品 ID。
///
/// The stack to put in the hand for this click. `None` keeps the id-only replay.
pub(super) fn stack(state: BlockState, item: i32, sneak: bool) -> Option<ItemStack> {
    if item == items::AIR || !state.is_a(mc_bamboo_data::BlockAncestor::AbstractCauldronBlock) {
        return None;
    }
    if item == items::POTION {
        return Some(with_set(
            item,
            kinds::POTION_CONTENTS,
            ComponentValue::PotionContents {
                potion: water_potion_id(),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            },
        ));
    }
    if item_is_tagged(item, item_tags::CAULDRON_CAN_REMOVE_DYE) {
        return Some(with_set(
            item,
            kinds::DYED_COLOR,
            ComponentValue::DyedColor(DYE_RGB),
        ));
    }
    if !sneak && state.is(blocks::WATER_CAULDRON) && item_is_tagged(item, item_tags::BANNERS) {
        let layer = BannerLayer {
            pattern: Holder::Id(base_pattern()),
            color: WHITE_DYE,
        };
        return Some(with_set(
            item,
            kinds::BANNER_PATTERNS,
            ComponentValue::BannerPatterns(vec![layer]),
        ));
    }
    None
}

fn with_set(item: i32, kind: i32, value: ComponentValue) -> ItemStack {
    let mut patch = ComponentPatch::new();
    patch.put(kind, PatchEntry::Set(value));
    let (added, removed) = patch.to_opaque();
    ItemStack {
        item,
        count: 1,
        added,
        removed,
    }
}

fn water_potion_id() -> Option<i32> {
    mc_bamboo_data::recipes::POTIONS
        .iter()
        .position(|name| *name == "minecraft:water")
        .and_then(|id| i32::try_from(id).ok())
}

fn base_pattern() -> i32 {
    mc_bamboo_data::registries::SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:banner_pattern")
        .and_then(|registry| {
            registry
                .entries
                .iter()
                .position(|name| *name == "minecraft:base")
        })
        .and_then(|id| i32::try_from(id).ok())
        .expect("minecraft:base is a vanilla banner pattern")
}
