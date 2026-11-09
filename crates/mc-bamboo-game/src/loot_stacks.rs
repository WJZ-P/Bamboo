//! 战利品表和游戏之间来回转换物品堆与组件。
//!
//! `LootStack.components` 里的值是官方 `Codec` 的 JSON 表示。转成物品堆时按官方
//! `JsonOps.convertTo(NbtOps, json)` 的规则先变成 NBT，再用组件存档编解码器
//! 解成 `ComponentPatch`。方块实体身上的组件反过来按 `NbtOps` → `JsonOps` 变成
//! 战利品表能读的 JSON。单个组件解不开时只丢掉那一项，其余照留。
//!
//! Converts item stacks and components between the loot table and the game. Official
//! codec JSON becomes NBT and then a typed component patch; block-entity components
//! go the other way. An entry that cannot be decoded is dropped with a warning.

use std::collections::BTreeMap;

use mc_bamboo_loot::{Components, LootStack};
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, PatchEntry,
        archive::{Dyn, components, read},
        cursor::Registry,
    },
    item::ItemStack,
};
use serde_json::{Map, Value};
use tracing::warn;

/// 把方块实体收集出的组件转成 `BLOCK_ENTITY` 参数里的 JSON 组件表。
///
/// 官方 `CopyComponentsFunction.BlockEntitySource` 从 `BlockEntity.collectComponents()`
/// 取组件；Bamboo 复用 `block_items::collect` 的那份 `ComponentPatch`，这里按存档
/// `Codec` 编成 JSON 供战利品函数读取。转换失败的组件只丢掉那一项。
///
/// Turns the components collected from a block entity into the JSON component map of
/// the `BLOCK_ENTITY` loot parameter. A value without a persistent codec is dropped.
pub fn component_json(patch: &ComponentPatch) -> Components {
    let mut out = BTreeMap::new();
    for (kind, entry) in patch.entries() {
        let PatchEntry::Set(value) = entry else {
            continue;
        };
        let Ok(name) = read::id_name(Registry::DATA_COMPONENT_TYPE, *kind) else {
            continue;
        };
        match components::encode_value(value, 0) {
            Ok(encoded) => {
                out.insert(name.to_owned(), dyn_to_json(&encoded));
            }
            Err(error) => warn!(
                component = name,
                %error,
                "skipping a block entity component without an archive codec"
            ),
        }
    }
    out
}

/// 官方 `NbtOps.convertTo(JsonOps, tag)`：数值、布尔字节、列表和映射都按 JSON 表示。
///
/// Vanilla `NbtOps.convertTo(JsonOps, tag)`: numbers, boolean bytes, lists and maps
/// in their JSON form.
fn dyn_to_json(value: &Dyn) -> Value {
    match value {
        Dyn::Byte(value) => Value::from(*value),
        Dyn::Short(value) => Value::from(*value),
        Dyn::Int(value) => Value::from(*value),
        Dyn::Long(value) => Value::from(*value),
        Dyn::Float(value) => Value::from(*value),
        Dyn::Double(value) => Value::from(*value),
        Dyn::Bool(value) => Value::Bool(*value),
        Dyn::String(value) => Value::String(value.clone()),
        Dyn::List(values) => Value::Array(values.iter().map(dyn_to_json).collect()),
        Dyn::Map(entries) => Value::Object(
            entries
                .iter()
                .map(|(name, value)| (name.clone(), dyn_to_json(value)))
                .collect::<Map<String, Value>>(),
        ),
        Dyn::ByteArray(values) => {
            Value::Array(values.iter().map(|value| Value::from(*value)).collect())
        }
        Dyn::IntArray(values) => {
            Value::Array(values.iter().map(|value| Value::from(*value)).collect())
        }
        Dyn::LongArray(values) => {
            Value::Array(values.iter().map(|value| Value::from(*value)).collect())
        }
    }
}

/// 把战利品表的一堆变成游戏里的物品堆。
///
/// 组件名带 `!` 前缀或值为 `null` 的项写成 `PatchEntry::Removed`。
///
/// Converts one loot stack into an in-game item stack. A `!` prefix or a null value
/// becomes `PatchEntry::Removed`.
pub fn loot_stack(stack: &LootStack) -> ItemStack {
    if stack.is_empty() {
        return ItemStack::empty();
    }
    let patch = loot_patch(&stack.components);
    let (added, removed) = patch.to_opaque();
    ItemStack {
        item: stack.item,
        count: stack.count,
        added,
        removed,
    }
}

/// 把 JSON 形式的组件补丁转成类型化的补丁。
///
/// 每一项独立处理：名字不在注册表里、JSON 转不成 NBT、或者存档编解码器解不开时
/// 只记一条警告，这一项不写进补丁，别的项照常保留。
///
/// Converts a JSON component patch into a typed patch. Each entry is independent: a
/// bad name, an unconvertible JSON value or a decode failure only drops that entry.
fn loot_patch(components: &mc_bamboo_loot::Components) -> ComponentPatch {
    let mut patch = ComponentPatch::new();
    let mut removed = Vec::new();
    for (name, value) in components {
        let (is_removed, name) = match name.strip_prefix('!') {
            Some(name) => (true, name),
            None => (false, name.as_str()),
        };
        let is_removed = is_removed || value.is_null();
        let Some(kind) = read::name_id(Registry::DATA_COMPONENT_TYPE, name) else {
            warn!(
                component = name,
                "skipping a loot component with an unknown name"
            );
            continue;
        };
        if is_removed {
            removed.push(kind);
            continue;
        }
        let tag = json_to_tag(value).to_tag();
        match components::decode_value(kind, &tag, 0) {
            Ok(value) => patch.put(kind, PatchEntry::Set(value)),
            Err(error) => warn!(
                component = name,
                %error,
                "skipping a loot component that the archive codec cannot decode"
            ),
        }
    }
    for kind in removed {
        patch.put(kind, PatchEntry::Removed);
    }
    patch
}

/// 官方 `JsonOps.convertTo(NbtOps, json)`。
///
/// 布尔写成字节；整数按能容纳它的最小 NBT 数值类型写；浮点数先试 `float`；
/// 数组写成 NBT 列表；`null` 写成 `TAG_End`（外层组件已经把它当成移除项）。
///
/// `JsonOps.convertTo(NbtOps, json)`: booleans are bytes, integers use the narrowest
/// NBT numeric tag that fits, floats use a float tag when round-tripping exactly,
/// arrays are lists, and null is an end tag.
fn json_to_tag(value: &Value) -> Dyn {
    match value {
        Value::Null => Dyn::Byte(0),
        Value::Bool(value) => Dyn::Bool(*value),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                if let Ok(value) = i8::try_from(value) {
                    return Dyn::Byte(value);
                }
                if let Ok(value) = i16::try_from(value) {
                    return Dyn::Short(value);
                }
                if let Ok(value) = i32::try_from(value) {
                    return Dyn::Int(value);
                }
                return Dyn::Long(value);
            }
            if let Some(value) = number.as_u64() {
                if let Ok(value) = i32::try_from(value) {
                    return Dyn::Int(value);
                }
                if let Ok(value) = i64::try_from(value) {
                    return Dyn::Long(value);
                }
            }
            if let Some(value) = number.as_f64() {
                let as_float = value as f32;
                if f64::from(as_float) == value {
                    return Dyn::Float(as_float);
                }
                return Dyn::Double(value);
            }
            Dyn::Double(0.0)
        }
        Value::String(value) => Dyn::String(value.clone()),
        Value::Array(values) => Dyn::List(values.iter().map(json_to_tag).collect()),
        Value::Object(entries) => Dyn::Map(
            entries
                .iter()
                .map(|(name, value)| (name.clone(), json_to_tag(value)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{items, registries::data_component_type as kinds};
    use mc_bamboo_nbt::Tag;
    use serde_json::json;

    use super::*;

    fn stack_with(components: Vec<(&str, Value)>) -> ItemStack {
        let mut loot = LootStack::new(items::DIAMOND_SWORD, 1);
        for (name, value) in components {
            loot.set_component(name, value);
        }
        loot_stack(&loot)
    }

    #[test]
    fn simple_component_values_survive_the_conversion() {
        let stack = stack_with(vec![
            ("minecraft:damage", json!(3)),
            ("minecraft:custom_name", json!("锋利的剑")),
            ("minecraft:unbreakable", json!({})),
        ]);
        let patch = stack.patch();
        assert!(matches!(
            patch.get(kinds::DAMAGE),
            Some(mc_bamboo_protocol::components::ComponentValue::Damage(3))
        ));
        assert!(matches!(
            patch.get(kinds::CUSTOM_NAME),
            Some(mc_bamboo_protocol::components::ComponentValue::CustomName(
                _
            ))
        ));
        assert!(matches!(
            patch.get(kinds::UNBREAKABLE),
            Some(mc_bamboo_protocol::components::ComponentValue::Unbreakable)
        ));
    }

    #[test]
    fn enchantment_maps_and_nested_items_survive_the_conversion() {
        let stack = stack_with(vec![
            ("minecraft:enchantments", json!({"minecraft:sharpness": 3})),
            (
                "minecraft:container",
                json!([{"slot": 0, "item": {"id": "minecraft:stone", "count": 4}}]),
            ),
        ]);
        let patch = stack.patch();
        assert!(patch.get(kinds::ENCHANTMENTS).is_some());
        assert!(patch.get(kinds::CONTAINER).is_some());
    }

    #[test]
    fn a_bad_component_does_not_affect_the_others() {
        let stack = stack_with(vec![
            ("minecraft:not_a_component", json!(1)),
            ("minecraft:damage", json!({"bad": "shape"})),
            ("minecraft:repair_cost", json!(7)),
        ]);
        let patch = stack.patch();
        assert!(patch.get(kinds::DAMAGE).is_none());
        assert!(matches!(
            patch.get(kinds::REPAIR_COST),
            Some(mc_bamboo_protocol::components::ComponentValue::RepairCost(
                7
            ))
        ));
    }

    #[test]
    fn removed_components_and_empty_stacks_are_preserved() {
        let stack = stack_with(vec![("!minecraft:damage", json!({}))]);
        assert!(stack.removed.contains(&kinds::DAMAGE));
        assert_eq!(loot_stack(&LootStack::empty()), ItemStack::empty());
    }

    #[test]
    fn json_numbers_follow_the_official_nbt_conversion() {
        assert_eq!(json_to_tag(&json!(3)).to_tag(), Tag::Byte(3));
        assert_eq!(json_to_tag(&json!(300)).to_tag(), Tag::Short(300));
        assert_eq!(json_to_tag(&json!(70_000)).to_tag(), Tag::Int(70_000));
        assert_eq!(
            json_to_tag(&json!(5_000_000_000_i64)).to_tag(),
            Tag::Long(5_000_000_000)
        );
        assert_eq!(json_to_tag(&json!(true)).to_tag(), Tag::Byte(1));
        assert_eq!(json_to_tag(&json!(1.5)).to_tag(), Tag::Float(1.5));
        assert_eq!(json_to_tag(&json!(1.1_f64)).to_tag(), Tag::Double(1.1_f64));
    }
}
