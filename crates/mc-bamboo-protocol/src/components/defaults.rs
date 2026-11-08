//! 每个物品的默认组件，官方 `Item.components()`。
//!
//! 生成的数据是官方 `DataComponentPatch.STREAM_CODEC` 编出来的字节，解码走的就是
//! [`ComponentPatch::read`]，所以这张表和线上的组件是同一套编解码。
//! 第一次用到时整张表一起解开，之后只是查下标。

use std::sync::LazyLock;

use mc_bamboo_data::item_components::{DEFAULTS, RANGES};

use crate::{
    codec::DecodeError,
    components::{ComponentPatch, ComponentValue, cursor::Cursor},
};

/// 一个空补丁，物品 ID 不认识时返回它。
static EMPTY: ComponentPatch = ComponentPatch::new();

/// 解开的默认组件，下标是物品的注册表 ID。
///
/// 生成的字节来自官方，`components` 对照会逐个物品检查，所以这里解不开只能是 bug。
/// 真解不开时留一个空补丁，不让世界线程崩在静态初始化里；单元测试会把这种情况抓出来。
static TABLE: LazyLock<Vec<ComponentPatch>> = LazyLock::new(|| {
    (0..RANGES.len())
        .map(|item| decode(item).unwrap_or_default())
        .collect()
});

/// 一个物品的默认组件。物品 ID 不认识时是空补丁。
pub fn of_item(item: i32) -> &'static ComponentPatch {
    usize::try_from(item)
        .ok()
        .and_then(|item| TABLE.get(item))
        .unwrap_or(&EMPTY)
}

/// 官方 `PatchedDataComponentMap.get`：补丁里设过就用补丁的，标了移除就当没有，否则看默认组件。
pub fn resolve(item: i32, patch: &ComponentPatch, kind: i32) -> Option<&ComponentValue> {
    match patch.entry(kind) {
        Some(super::PatchEntry::Set(value)) => Some(value),
        Some(super::PatchEntry::Removed) => None,
        None => of_item(item).get(kind),
    }
}

/// 解开一个物品的默认组件，要求刚好用完它那一段字节。
fn decode(item: usize) -> Result<ComponentPatch, DecodeError> {
    let (start, end) = RANGES[item];
    let mut cursor = Cursor::new(&DEFAULTS[start as usize..end as usize]);
    let patch = ComponentPatch::read(&mut cursor)?;
    cursor.finish()?;
    Ok(patch)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{items, registries::data_component_type as kinds};

    use super::*;
    use crate::components::PatchEntry;

    #[test]
    fn every_item_decodes_and_encodes_back_to_the_generated_bytes() {
        for (item, &(start, end)) in RANGES.iter().enumerate() {
            let patch = decode(item).unwrap_or_else(|error| {
                panic!("{} 的默认组件解不开：{error}", items::ITEMS[item].name)
            });
            let mut again = Vec::new();
            patch.write(&mut again);
            assert_eq!(
                again,
                &DEFAULTS[start as usize..end as usize],
                "{} 的默认组件写回去不一样",
                items::ITEMS[item].name
            );
        }
    }

    #[test]
    fn the_table_covers_every_item() {
        assert_eq!(RANGES.len(), items::ITEMS.len());
        // 连空气都有官方 `COMMON_ITEM_COMPONENTS` 那一套。
        assert_eq!(
            of_item(items::AIR).get(kinds::MAX_STACK_SIZE),
            Some(&ComponentValue::MaxStackSize(64))
        );
        assert!(
            TABLE.iter().all(|patch| !patch.entries().is_empty()),
            "每个物品都该有默认组件"
        );
    }

    #[test]
    fn the_common_components_are_there() {
        let stone = of_item(items::STONE);
        assert_eq!(
            stone.get(kinds::MAX_STACK_SIZE),
            Some(&ComponentValue::MaxStackSize(64))
        );
        assert_eq!(stone.get(kinds::RARITY), Some(&ComponentValue::Rarity(0)));
        assert!(stone.get(kinds::MAX_DAMAGE).is_none());

        let sword = of_item(items::DIAMOND_SWORD);
        assert_eq!(
            sword.get(kinds::MAX_STACK_SIZE),
            Some(&ComponentValue::MaxStackSize(1))
        );
        assert!(matches!(
            sword.get(kinds::MAX_DAMAGE),
            Some(&ComponentValue::MaxDamage(_))
        ));
        assert!(matches!(
            sword.get(kinds::TOOL),
            Some(&ComponentValue::Tool { .. })
        ));
    }

    #[test]
    fn an_unknown_item_has_no_components() {
        assert_eq!(of_item(-1), &ComponentPatch::new());
        assert_eq!(of_item(items::ITEMS.len() as i32), &ComponentPatch::new());
    }

    #[test]
    fn resolving_prefers_the_patch_and_honours_removals() {
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::MAX_STACK_SIZE,
            PatchEntry::Set(ComponentValue::MaxStackSize(9)),
        );
        assert_eq!(
            resolve(items::STONE, &patch, kinds::MAX_STACK_SIZE),
            Some(&ComponentValue::MaxStackSize(9))
        );
        // 没在补丁里的键看默认组件。
        assert_eq!(
            resolve(items::DIAMOND_SWORD, &patch, kinds::MAX_DAMAGE),
            of_item(items::DIAMOND_SWORD).get(kinds::MAX_DAMAGE)
        );

        let mut patch = ComponentPatch::new();
        patch.put(kinds::MAX_DAMAGE, PatchEntry::Removed);
        assert_eq!(
            resolve(items::DIAMOND_SWORD, &patch, kinds::MAX_DAMAGE),
            None
        );
    }
}
