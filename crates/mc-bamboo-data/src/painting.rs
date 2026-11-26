//! 画的变体静态表（`minecraft:painting_variant`）：按同步注册表顺序记录尺寸（宽、高，单位格），
//! 并提供按名 / 按 ID 查询和 `#minecraft:placeable` 标签顺序。
//!
//! Static table for `minecraft:painting_variant`: dimensions (width, height in blocks)
//! indexed by synced registry id, name/id lookup, and `#minecraft:placeable` order.

use crate::{registries, tags};

/// 官方 `minecraft:painting_variant` 的注册表名。
const REGISTRY_NAME: &str = "minecraft:painting_variant";
/// 可摆放画变体的标签名。
const PLACEABLE_TAG: &str = "minecraft:placeable";

/// 每个画变体的 `(宽, 高)`（单位：方块），下标与 `SYNCED_REGISTRIES` 中
/// `minecraft:painting_variant` 一一对应（共 51 种）。
///
/// `(width, height)` in blocks for each painting variant, indexed by the synced
/// `minecraft:painting_variant` registry order (51 entries).
pub const SIZES: &[(i32, i32)] = &[
    (1, 1), //  0: minecraft:alban
    (1, 1), //  1: minecraft:aztec
    (1, 1), //  2: minecraft:aztec2
    (3, 4), //  3: minecraft:backyard
    (2, 2), //  4: minecraft:baroque
    (1, 1), //  5: minecraft:bomb
    (3, 3), //  6: minecraft:bouquet
    (4, 4), //  7: minecraft:burning_skull
    (2, 2), //  8: minecraft:bust
    (3, 3), //  9: minecraft:cavebird
    (4, 2), // 10: minecraft:changing
    (3, 3), // 11: minecraft:cotan
    (2, 1), // 12: minecraft:courbet
    (2, 1), // 13: minecraft:creebet
    (3, 3), // 14: minecraft:dennis
    (4, 3), // 15: minecraft:donkey_kong
    (2, 2), // 16: minecraft:earth
    (3, 3), // 17: minecraft:endboss
    (3, 3), // 18: minecraft:fern
    (4, 2), // 19: minecraft:fighters
    (4, 2), // 20: minecraft:finding
    (2, 2), // 21: minecraft:fire
    (1, 2), // 22: minecraft:graham
    (2, 2), // 23: minecraft:humble
    (1, 1), // 24: minecraft:kebab
    (4, 2), // 25: minecraft:lowmist
    (2, 2), // 26: minecraft:match
    (1, 1), // 27: minecraft:meditative
    (4, 4), // 28: minecraft:orb
    (3, 3), // 29: minecraft:owlemons
    (4, 2), // 30: minecraft:passage
    (4, 4), // 31: minecraft:pigscene
    (1, 1), // 32: minecraft:plant
    (4, 4), // 33: minecraft:pointer
    (3, 4), // 34: minecraft:pond
    (2, 1), // 35: minecraft:pool
    (1, 2), // 36: minecraft:prairie_ride
    (2, 1), // 37: minecraft:sea
    (4, 3), // 38: minecraft:skeleton
    (2, 2), // 39: minecraft:skull_and_roses
    (2, 2), // 40: minecraft:stage
    (3, 3), // 41: minecraft:sunflowers
    (2, 1), // 42: minecraft:sunset
    (3, 3), // 43: minecraft:tides
    (4, 4), // 44: minecraft:unpacked
    (2, 2), // 45: minecraft:void
    (1, 2), // 46: minecraft:wanderer
    (1, 1), // 47: minecraft:wasteland
    (2, 2), // 48: minecraft:water
    (2, 2), // 49: minecraft:wind
    (2, 2), // 50: minecraft:wither
];

/// 默认画变体 ID（`VariantUtils.getAny`：注册表第一项 `minecraft:alban`）。
///
/// Default painting variant id (`VariantUtils.getAny`: registry entry 0, `minecraft:alban`).
pub const DEFAULT_VARIANT: i32 = 0;

/// 注册表里全部画变体的名称列表。
///
/// Names of all painting variants in registry order.
pub fn entries() -> &'static [&'static str] {
    registries::SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == REGISTRY_NAME)
        .map_or(&[], |registry| registry.entries)
}

/// 按注册表 ID 取 `(宽, 高)`；越界返回 `None`。
///
/// Return `(width, height)` for a registry id, or `None` if out of range.
pub fn size(id: i32) -> Option<(i32, i32)> {
    usize::try_from(id).ok().and_then(|i| SIZES.get(i).copied())
}

/// 变体的面积（`width * height`）；越界返回 1。
///
/// Variant area (`width * height`), or 1 if out of range.
pub fn area(id: i32) -> i32 {
    size(id).map_or(1, |(w, h)| w * h)
}

/// 按注册表 ID 取命名空间名称（如 `"minecraft:kebab"`）。
///
/// Registry name for `id` (e.g. `"minecraft:kebab"`).
pub fn name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|i| entries().get(i).copied())
}

/// 按名字查注册表 ID；支持省略默认命名空间 `minecraft:`。
///
/// Look up a registry id by name; accepts both `"minecraft:kebab"` and `"kebab"`.
pub fn id_of(name: &str) -> Option<i32> {
    let entries = entries();
    if let Some(index) = entries.iter().position(|&entry| entry == name) {
        return Some(index as i32);
    }
    if !name.contains(':') {
        let qualified = format!("minecraft:{name}");
        return entries
            .iter()
            .position(|&entry| entry == qualified)
            .map(|index| index as i32);
    }
    None
}

/// `#minecraft:painting_variant/placeable` 标签里的变体 ID，保持官方声明顺序。
///
/// Variant ids in `#minecraft:painting_variant/placeable`, in official declaration order.
pub fn placeable_ids() -> &'static [i32] {
    tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == REGISTRY_NAME)
        .and_then(|group| group.tags.iter().find(|tag| tag.name == PLACEABLE_TAG))
        .map_or(&[], |tag| tag.ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_align_with_synced_registry() {
        let names = entries();
        assert_eq!(names.len(), SIZES.len());
        assert_eq!(names.len(), 51);
        assert_eq!(name(0), Some("minecraft:alban"));
        assert_eq!(id_of("minecraft:kebab"), Some(24));
        assert_eq!(id_of("kebab"), Some(24));
        assert_eq!(size(24), Some((1, 1)));
        assert_eq!(size(15), Some((4, 3)));
    }

    #[test]
    fn placeable_tag_has_47_variants_excluding_elements() {
        let ids = placeable_ids();
        assert_eq!(ids.len(), 47);
        assert_eq!(ids[0], id_of("minecraft:kebab").unwrap());
        for hidden in [
            "minecraft:earth",
            "minecraft:wind",
            "minecraft:water",
            "minecraft:fire",
        ] {
            let id = id_of(hidden).unwrap();
            assert!(!ids.contains(&id), "{hidden} should not be placeable");
        }
    }
}
