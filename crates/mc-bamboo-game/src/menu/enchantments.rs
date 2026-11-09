//! 附魔注册表的数据，给铁砧和附魔台共用。
//!
//! 数字全部来自生成的数据包 JSON 和生成的注册表、标签，不在这里写死版本相关的表。
//! JSON 只挑需要的几个整数字段和标签名，和 `grindstone.rs` 的做法一样。

use std::sync::OnceLock;

use mc_bamboo_data::{item_tag, loot, registries::SYNCED_REGISTRIES, tags};

const ENCHANTMENT_REGISTRY: &str = "minecraft:enchantment";

/// 一条附魔定义在官方 JSON 里能被菜单用到的那部分。
#[derive(Debug, Clone, Copy)]
pub(super) struct Definition {
    /// `anvil_cost`：铁砧合并附魔时的单价。
    pub anvil_cost: i32,
    /// `weight`：附魔台加权抽取的权重。
    pub weight: i32,
    /// `max_level`。
    pub max_level: i32,
    /// `min_cost` 的 base / per_level_above_first。
    pub min_cost: (i32, i32),
    /// `max_cost` 的 base / per_level_above_first。
    pub max_cost: (i32, i32),
    /// `exclusive_set` 标签里的附魔 ID；没有这个字段时是空表。
    pub exclusive: &'static [i32],
    /// `supported_items` 里的物品 ID。
    pub supported: &'static [i32],
    /// `primary_items` 里的物品 ID；没有这个字段时 `supported` 全是 primary。
    pub primary: Option<&'static [i32]>,
}

impl Definition {
    /// `Enchantment.Cost.calculate(level)`。
    pub fn min_at(self, level: i32) -> i32 {
        let (base, per) = self.min_cost;
        base + per * (level - 1)
    }

    /// `Enchantment.Cost.calculate(level)`。
    pub fn max_at(self, level: i32) -> i32 {
        let (base, per) = self.max_cost;
        base + per * (level - 1)
    }
}

/// `minecraft:in_enchanting_table` 标签里的附魔 ID，顺序照标签。
pub(super) fn in_table() -> &'static [i32] {
    enchantment_tag("minecraft:in_enchanting_table")
}

/// 注册表 ID → 定义。下标越界时给一份全 0 的定义。
pub(super) fn get(id: i32) -> Definition {
    static TABLE: OnceLock<Vec<Definition>> = OnceLock::new();
    let table = TABLE.get_or_init(build);
    usize::try_from(id)
        .ok()
        .and_then(|id| table.get(id))
        .copied()
        .unwrap_or(Definition {
            anvil_cost: 0,
            weight: 0,
            max_level: 1,
            min_cost: (0, 0),
            max_cost: (0, 0),
            exclusive: &[],
            supported: &[],
            primary: None,
        })
}

/// `Enchantment.areCompatible`：相同 ID 或任一 `exclusive_set` 含对方时不兼容。
pub(super) fn compatible(a: i32, b: i32) -> bool {
    a != b && !get(a).exclusive.contains(&b) && !get(b).exclusive.contains(&a)
}

/// `Enchantment.canEnchant`：只看 `supported_items`。
pub(super) fn can_enchant(id: i32, item: i32) -> bool {
    get(id).supported.contains(&item)
}

/// `Enchantment.isPrimaryItem`：没有 `primary_items` 时所有 supported 都是 primary。
pub(super) fn is_primary(id: i32, item: i32) -> bool {
    let definition = get(id);
    match definition.primary {
        Some(primary) => primary.contains(&item),
        None => definition.supported.contains(&item),
    }
}

fn build() -> Vec<Definition> {
    let names = SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == ENCHANTMENT_REGISTRY)
        .map(|registry| registry.entries)
        .unwrap_or(&[]);
    let mut table = vec![
        Definition {
            anvil_cost: 0,
            weight: 0,
            max_level: 1,
            min_cost: (0, 0),
            max_cost: (0, 0),
            exclusive: &[],
            supported: &[],
            primary: None,
        };
        names.len()
    ];
    for (name, json) in loot::ENCHANTMENTS {
        let Some(id) = names.iter().position(|entry| entry == name) else {
            continue;
        };
        table[id] = Definition {
            anvil_cost: field_int(json, "\"anvil_cost\""),
            weight: field_int(json, "\"weight\""),
            max_level: field_int(json, "\"max_level\""),
            min_cost: cost(json, "\"min_cost\""),
            max_cost: cost(json, "\"max_cost\""),
            exclusive: enchantment_tag_of(json, "\"exclusive_set\""),
            supported: item_tag_of(json, "\"supported_items\""),
            primary: field_string(json, "\"primary_items\"")
                .map(|name| item_tag(name.trim_start_matches('#')).unwrap_or(&[])),
        };
    }
    table
}

fn enchantment_tag(name: &str) -> &'static [i32] {
    tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == ENCHANTMENT_REGISTRY)
        .and_then(|group| group.tags.iter().find(|tag| tag.name == name))
        .map(|tag| tag.ids)
        .unwrap_or(&[])
}

fn enchantment_tag_of(json: &str, key: &str) -> &'static [i32] {
    field_string(json, key)
        .map(|name| enchantment_tag(name.trim_start_matches('#')))
        .unwrap_or(&[])
}

fn item_tag_of(json: &str, key: &str) -> &'static [i32] {
    field_string(json, key)
        .map(|name| item_tag(name.trim_start_matches('#')).unwrap_or(&[]))
        .unwrap_or(&[])
}

/// 从 JSON 里读一个整数字段；字段不存在时是 0。
fn field_int(json: &str, key: &str) -> i32 {
    let Some(at) = json.find(key) else {
        return 0;
    };
    number_after(&json[at + key.len()..])
}

/// 读 JSON 里的一个字符串字段；字段不存在时是 `None`。
fn field_string<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let at = json.find(key)?;
    let rest = &json[at + key.len()..];
    let start = rest.find(": \"")? + 3;
    let value = &rest[start..];
    let end = value.find('"')?;
    Some(&value[..end])
}

/// 从 `{"base":..,"per_level_above_first":..}` 里读两个整数。
fn cost(json: &str, key: &str) -> (i32, i32) {
    let Some(at) = json.find(key) else {
        return (0, 0);
    };
    let rest = &json[at + key.len()..];
    let rest = rest.strip_prefix("\\\"").unwrap_or(rest);
    let close = rest.find('}').unwrap_or(rest.len());
    let body = &rest[..close];
    (
        field_int(body, "\"base\""),
        field_int(body, "\"per_level_above_first\""),
    )
}

fn number_after(text: &str) -> i32 {
    let number: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit() && *c != '-')
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    number.parse().unwrap_or(0)
}

#[allow(dead_code)]
/// 物品默认组件里的 `enchantable.value`，不在补丁里时来自静态表。
pub(super) fn enchantability(item: i32) -> Option<i32> {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};
    match defaults::of_item(item).get(kinds::ENCHANTABLE) {
        Some(ComponentValue::Enchantable(value)) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{items, registries};

    use super::*;

    #[test]
    fn sharpness_comes_from_the_generated_json() {
        let id = registries::SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == ENCHANTMENT_REGISTRY)
            .and_then(|registry| {
                registry
                    .entries
                    .iter()
                    .position(|e| *e == "minecraft:sharpness")
            })
            .expect("sharpness is registered") as i32;
        let sharpness = get(id);
        assert_eq!(sharpness.anvil_cost, 1);
        assert_eq!(sharpness.weight, 10);
        assert_eq!(sharpness.max_level, 5);
        assert_eq!(sharpness.min_at(3), 23);
        assert!(sharpness.supported.contains(&items::DIAMOND_SWORD));
        assert!(!compatible(id, id));
        assert!(can_enchant(id, items::DIAMOND_SWORD));
    }

    #[test]
    fn an_enchantable_item_has_its_default_value() {
        assert_eq!(enchantability(items::DIAMOND_SWORD), Some(10));
        assert_eq!(enchantability(items::BOOK), Some(1));
        assert_eq!(enchantability(items::STONE), None);
    }
}
