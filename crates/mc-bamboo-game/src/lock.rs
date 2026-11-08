//! 容器锁，官方 `LockCode.canUnlock` 和 `ItemPredicate.test`。
//!
//! 空锁（没有 `lock`，或谓词是空的）什么都能开。旁观者在调用方放行。
//! 谓词里 `items`、`count`、`components` 按官方比较。`predicates` 里目前实现了
//! `damage`、`custom_data` 和“组件在不在”；其余谓词对不上，锁就打不开。
//!
//! Container locks, `LockCode.canUnlock` and `ItemPredicate.test`.
//!
//! An absent or empty lock opens for anything. The caller lets spectators through.
//! `items`, `count` and `components` follow vanilla. Among `predicates`, `damage`,
//! `custom_data` and “component present” are implemented; any other predicate fails
//! the unlock.

use mc_bamboo_data::{
    item_tag,
    items::ITEMS,
    registries::data_component_type::{self as kinds},
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, archive::components::same_decoded, defaults},
    item::ItemStack,
};

use crate::{block_items, region::Region};
use mc_bamboo_core::BlockPos;

/// 这些名字是谓词类型。和组件重名时谓词优先，官方 `either` 也是左边先赢。
///
/// Predicate type names. A shared name prefers the predicate, matching `either`'s left side.
const PREDICATE_TYPES: &[&str] = &[
    "minecraft:damage",
    "minecraft:enchantments",
    "minecraft:stored_enchantments",
    "minecraft:potion_contents",
    "minecraft:custom_data",
    "minecraft:container",
    "minecraft:bundle_contents",
    "minecraft:firework_explosion",
    "minecraft:fireworks",
    "minecraft:writable_book_content",
    "minecraft:written_book_content",
    "minecraft:attribute_modifiers",
    "minecraft:trim",
    "minecraft:jukebox_playable",
    "minecraft:villager/variant",
];

/// 这一格的锁能不能被 `stack` 打开。没有锁就是能开。
///
/// Whether `stack` opens the lock on this block. No lock means yes.
pub(crate) fn unlocks(region: &Region, pos: BlockPos, stack: &ItemStack) -> bool {
    let Some(lock) = block_items::lock_tag(region, pos) else {
        return true;
    };
    predicate_matches(&lock, stack)
}

/// 官方 `ItemPredicate.test`。
///
/// `ItemPredicate.test`.
fn predicate_matches(lock: &Tag, stack: &ItemStack) -> bool {
    let Tag::Compound(entries) = lock else {
        return false;
    };
    let patch = stack.patch();
    items_match(field(entries, "items"), stack.item)
        && count_match(field(entries, "count"), stack.count)
        && exact_components(field(entries, "components"), stack.item, &patch)
        && predicates_match(field(entries, "predicates"), stack.item, &patch)
}

fn items_match(items: Option<&Tag>, item: i32) -> bool {
    let Some(items) = items else {
        return true;
    };
    holder_contains(items, item)
}

fn holder_contains(tag: &Tag, item: i32) -> bool {
    match tag {
        Tag::String(name) => one_item(name, item),
        Tag::List(values) => values.iter().any(|value| match value {
            Tag::String(name) => one_item(name, item),
            _ => false,
        }),
        _ => false,
    }
}

fn one_item(name: &str, item: i32) -> bool {
    if let Some(tag) = name.strip_prefix('#') {
        return item_tag(&qualify(tag)).is_some_and(|ids| ids.contains(&item));
    }
    item_id(&qualify(name)) == Some(item)
}

fn count_match(count: Option<&Tag>, actual: i32) -> bool {
    bounds(count).matches(actual)
}

fn exact_components(components: Option<&Tag>, item: i32, patch: &ComponentPatch) -> bool {
    let Some(Tag::Compound(entries)) = components else {
        return components.is_none();
    };
    entries.iter().all(|(name, expected)| {
        let Some(kind) = component_id(name) else {
            return false;
        };
        defaults::resolve(item, patch, kind)
            .is_some_and(|actual| same_decoded(kind, expected, actual))
    })
}

fn predicates_match(predicates: Option<&Tag>, item: i32, patch: &ComponentPatch) -> bool {
    let Some(Tag::Compound(entries)) = predicates else {
        return predicates.is_none();
    };
    entries
        .iter()
        .all(|(name, body)| predicate_one(name, body, item, patch))
}

fn predicate_one(name: &str, body: &Tag, item: i32, patch: &ComponentPatch) -> bool {
    let name = qualify(name);
    if PREDICATE_TYPES.contains(&name.as_str()) {
        return match name.as_str() {
            "minecraft:damage" => damage_matches(body, item, patch),
            "minecraft:custom_data" => custom_data_matches(body, item, patch),
            _ => false,
        };
    }
    let Some(kind) = component_id(&name) else {
        return false;
    };
    defaults::resolve(item, patch, kind).is_some()
}

/// 官方 `DamagePredicate.matches`：没有 `damage` 组件就是对不上。
///
/// `DamagePredicate.matches`: a missing `damage` component does not match.
fn damage_matches(body: &Tag, item: i32, patch: &ComponentPatch) -> bool {
    let Tag::Compound(entries) = body else {
        return false;
    };
    let Some(ComponentValue::Damage(damage)) = defaults::resolve(item, patch, kinds::DAMAGE) else {
        return false;
    };
    let max = match defaults::resolve(item, patch, kinds::MAX_DAMAGE) {
        Some(ComponentValue::MaxDamage(max)) => *max,
        _ => 0,
    };
    bounds(field(entries, "durability")).matches(max - damage)
        && bounds(field(entries, "damage")).matches(*damage)
}

/// 官方 `CustomDataPredicate`：复合标签按键部分匹配，多余的键可以有。
///
/// `CustomDataPredicate`: a compound matches by key. Extra keys on the item are fine.
fn custom_data_matches(body: &Tag, item: i32, patch: &ComponentPatch) -> bool {
    let expected = match body {
        Tag::String(text) => match mc_bamboo_nbt::parse_snbt(text) {
            Ok(tag) => tag,
            Err(_) => return false,
        },
        other => other.clone(),
    };
    let Some(ComponentValue::CustomData(actual)) =
        defaults::resolve(item, patch, kinds::CUSTOM_DATA)
    else {
        return false;
    };
    contains(&Tag::Compound(actual.clone()), &expected)
}

fn contains(actual: &Tag, expected: &Tag) -> bool {
    match (actual, expected) {
        (Tag::Compound(actual), Tag::Compound(expected)) => {
            expected.iter().all(|(key, expected)| {
                actual
                    .iter()
                    .find(|(name, _)| name == key)
                    .is_some_and(|(_, actual)| contains(actual, expected))
            })
        }
        (Tag::List(actual), Tag::List(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| contains(actual, expected))
        }
        _ => actual == expected,
    }
}

struct Bounds {
    min: Option<i32>,
    max: Option<i32>,
}

impl Bounds {
    fn matches(&self, value: i32) -> bool {
        self.min.is_none_or(|min| value >= min) && self.max.is_none_or(|max| value <= max)
    }
}

fn bounds(tag: Option<&Tag>) -> Bounds {
    match tag {
        None => Bounds {
            min: None,
            max: None,
        },
        Some(Tag::Compound(entries)) => Bounds {
            min: field(entries, "min").and_then(int_value),
            max: field(entries, "max").and_then(int_value),
        },
        Some(other) => int_value(other).map_or(
            Bounds {
                min: Some(i32::MAX),
                max: Some(i32::MIN),
            },
            |point| Bounds {
                min: Some(point),
                max: Some(point),
            },
        ),
    }
}

fn int_value(tag: &Tag) -> Option<i32> {
    match tag {
        Tag::Byte(value) => Some(i32::from(*value)),
        Tag::Short(value) => Some(i32::from(*value)),
        Tag::Int(value) => Some(*value),
        Tag::Long(value) => i32::try_from(*value).ok(),
        _ => None,
    }
}

fn field<'a>(entries: &'a [(String, Tag)], name: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn qualify(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}

fn item_id(name: &str) -> Option<i32> {
    ITEMS
        .iter()
        .position(|item| item.name == name)
        .map(|id| id as i32)
}

fn component_id(name: &str) -> Option<i32> {
    let name = qualify(name);
    kinds::NAMES
        .iter()
        .position(|entry| *entry == name)
        .map(|id| id as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::items;

    fn named(name: &str) -> Tag {
        Tag::Compound(vec![("items".to_owned(), Tag::String(name.to_owned()))])
    }

    #[test]
    fn an_empty_predicate_matches_anything() {
        let stack = ItemStack::new(items::STONE, 1);
        assert!(predicate_matches(&Tag::Compound(Vec::new()), &stack));
    }

    #[test]
    fn the_item_set_and_count_both_have_to_match() {
        let hook = ItemStack::new(items::TRIPWIRE_HOOK, 1);
        let stone = ItemStack::new(items::STONE, 1);
        let lock = named("minecraft:tripwire_hook");
        assert!(predicate_matches(&lock, &hook));
        assert!(!predicate_matches(&lock, &stone));

        let counted = Tag::Compound(vec![
            (
                "items".to_owned(),
                Tag::String("minecraft:stone".to_owned()),
            ),
            ("count".to_owned(), Tag::Int(2)),
        ]);
        assert!(!predicate_matches(&counted, &stone));
        assert!(predicate_matches(
            &counted,
            &ItemStack::new(items::STONE, 2)
        ));
    }
}
