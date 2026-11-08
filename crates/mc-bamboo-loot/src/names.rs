//! 注册表名字和数字 ID 之间的换算。
//!
//! 战利品表的数据包里到处是 `"minecraft:stone"` 这样的名字，官方的 ID 就是注册表
//! 里的位置。名字写错时必须报错，不能悄悄变成 0，所以这里所有查找都返回 `Option`
//! 或 `Result`。

use std::collections::HashMap;
use std::sync::OnceLock;

use mc_bamboo_data::{blocks, items, loot, registries, tags};
use serde_json::Value;

use crate::Result;
use crate::json::{holder_name, invalid, object, opt_i32, required, string};

/// 战利品表会引用到的注册表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registry {
    Item,
    Block,
    EntityType,
    Enchantment,
    DamageType,
    Biome,
    MobEffect,
    Potion,
    Instrument,
}

impl Registry {
    /// 数据包里标签组用的注册表名字。
    fn tag_group(self) -> &'static str {
        match self {
            Registry::Item => "minecraft:item",
            Registry::Block => "minecraft:block",
            Registry::EntityType => "minecraft:entity_type",
            Registry::Enchantment => "minecraft:enchantment",
            Registry::DamageType => "minecraft:damage_type",
            Registry::Biome => "minecraft:worldgen/biome",
            Registry::MobEffect => "minecraft:mob_effect",
            Registry::Potion => "minecraft:potion",
            Registry::Instrument => "minecraft:instrument",
        }
    }

    /// 单个条目的名字。
    fn id(self, name: &str) -> Option<i32> {
        match self {
            Registry::Item => item_id(name),
            Registry::Block => block_id(name),
            Registry::EntityType => id_in(registries::entity_type::NAMES, name),
            Registry::Enchantment => enchantment_id(name),
            Registry::DamageType => synced_id("minecraft:damage_type", name),
            Registry::Biome => id_in(registries::biome::NAMES, name),
            Registry::MobEffect => id_in(registries::mob_effect::NAMES, name),
            Registry::Potion => id_in(registries::potion::NAMES, name),
            Registry::Instrument => synced_id("minecraft:instrument", name),
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Registry::Item => "item",
            Registry::Block => "block",
            Registry::EntityType => "entity type",
            Registry::Enchantment => "enchantment",
            Registry::DamageType => "damage type",
            Registry::Biome => "biome",
            Registry::MobEffect => "mob effect",
            Registry::Potion => "potion",
            Registry::Instrument => "instrument",
        }
    }
}

fn id_in(names: &[&str], name: &str) -> Option<i32> {
    names
        .iter()
        .position(|entry| *entry == name)
        .map(|index| index as i32)
}

fn synced_id(registry: &str, name: &str) -> Option<i32> {
    let synced = registries::SYNCED_REGISTRIES
        .iter()
        .find(|entry| entry.name == registry)?;
    id_in(synced.entries, name)
}

fn item_map() -> &'static HashMap<&'static str, i32> {
    static MAP: OnceLock<HashMap<&'static str, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        items::ITEMS
            .iter()
            .enumerate()
            .map(|(index, item)| (item.name, index as i32))
            .collect()
    })
}

fn block_map() -> &'static HashMap<&'static str, i32> {
    static MAP: OnceLock<HashMap<&'static str, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        blocks::BLOCKS
            .iter()
            .enumerate()
            .map(|(index, block)| (block.name, index as i32))
            .collect()
    })
}

/// 物品名 → 物品注册表 ID。
pub fn item_id(name: &str) -> Option<i32> {
    item_map().get(name).copied()
}

/// 方块名 → 方块注册表 ID。
pub fn block_id(name: &str) -> Option<i32> {
    block_map().get(name).copied()
}

pub fn item_name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|index| items::ITEMS.get(index))
        .map(|item| item.name)
}

pub fn block_name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|index| blocks::BLOCKS.get(index))
        .map(|block| block.name)
}

pub fn enchantment_name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|index| loot::ENCHANTMENTS.get(index))
        .map(|entry| entry.0)
}

pub fn entity_type_name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|index| registries::entity_type::NAMES.get(index))
        .copied()
}

pub fn potion_name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|index| registries::potion::NAMES.get(index))
        .copied()
}

pub fn mob_effect_name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|index| registries::mob_effect::NAMES.get(index))
        .copied()
}

pub fn instrument_name(id: i32) -> Option<&'static str> {
    registries::SYNCED_REGISTRIES
        .iter()
        .find(|entry| entry.name == "minecraft:instrument")
        .and_then(|entry| {
            usize::try_from(id)
                .ok()
                .and_then(|index| entry.entries.get(index).copied())
        })
}

pub fn biome_id(name: &str) -> Option<i32> {
    id_in(registries::biome::NAMES, name)
}

pub fn entity_type_id(name: &str) -> Option<i32> {
    id_in(registries::entity_type::NAMES, name)
}

pub fn enchantment_id(name: &str) -> Option<i32> {
    enchantment_map().get(name).copied()
}

fn enchantment_map() -> &'static HashMap<&'static str, i32> {
    static MAP: OnceLock<HashMap<&'static str, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        loot::ENCHANTMENTS
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.0, index as i32))
            .collect()
    })
}

/// 一个标签组里某个标签的条目 ID，顺序和官方一样。
pub fn tag_ids(registry: &str, name: &str) -> Option<&'static [i32]> {
    let group = tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == registry)?;
    group
        .tags
        .iter()
        .find(|tag| tag.name == name)
        .map(|tag| tag.ids)
}

/// 解析 `HolderSet`：单个名字、`#标签`、或者它们的数组。
///
/// 返回条目的注册表 ID，标签按数据包里的顺序展开。
pub fn holder_set(value: &Value, registry: Registry, what: &str) -> Result<Vec<i32>> {
    let mut result = Vec::new();
    for raw in super::json::string_or_list(value, what)? {
        let (name, is_tag) = holder_name(raw);
        if is_tag {
            let ids = tag_ids(registry.tag_group(), name).ok_or_else(|| {
                invalid(format!(
                    "unknown {} tag `{raw}` in {what}",
                    registry.describe()
                ))
            })?;
            result.extend_from_slice(ids);
        } else {
            let id = registry.id(name).ok_or_else(|| {
                invalid(format!("unknown {} `{raw}` in {what}", registry.describe()))
            })?;
            result.push(id);
        }
    }
    Ok(result)
}

/// 可选的 `HolderSet`。
pub fn opt_holder_set(
    object: &serde_json::Map<String, Value>,
    name: &str,
    registry: Registry,
    what: &str,
) -> Result<Option<Vec<i32>>> {
    match crate::json::field(object, name) {
        Some(value) => holder_set(value, registry, what).map(Some),
        None => Ok(None),
    }
}

/// 解析单个名字，不接受标签。
pub fn single_registry_id(value: &Value, registry: Registry, what: &str) -> Result<i32> {
    let raw = string(value, what)?;
    if raw.starts_with('#') {
        return Err(invalid(format!("{what} must not be a tag")));
    }
    registry
        .id(raw)
        .ok_or_else(|| invalid(format!("unknown {} `{raw}` in {what}", registry.describe())))
}

/// 附魔定义里用到的部分，从生成器导出的官方 JSON 里读出来。
#[derive(Debug)]
pub struct EnchantmentDef {
    pub name: &'static str,
    pub weight: i32,
    pub min_level: i32,
    pub max_level: i32,
    /// `min_cost` 的 `base` 和 `per_level_above_first`。
    pub min_cost: (i32, i32),
    pub max_cost: (i32, i32),
    pub supported_items: Vec<i32>,
    pub primary_items: Option<Vec<i32>>,
    pub exclusive_set: Option<Vec<i32>>,
    /// 能附上这个附魔的装备槽组，官方 `EquipmentSlotGroup` 的名字，没有 `minecraft:` 前缀。
    pub slots: Vec<String>,
}

impl EnchantmentDef {
    /// `Enchantment.Cost.calculate(level)`。
    pub fn min_cost_at(&self, level: i32) -> i32 {
        self.min_cost.0 + self.min_cost.1 * (level - 1)
    }

    pub fn max_cost_at(&self, level: i32) -> i32 {
        self.max_cost.0 + self.max_cost.1 * (level - 1)
    }

    /// `Enchantment.isSupportedItem`。
    pub fn is_supported(&self, item: i32) -> bool {
        self.supported_items.contains(&item)
    }

    /// `Enchantment.isPrimaryItem`：支持这个物品，并且没写 `primary_items` 或者写在里面。
    pub fn is_primary(&self, item: i32) -> bool {
        self.is_supported(item)
            && self
                .primary_items
                .as_ref()
                .is_none_or(|items| items.contains(&item))
    }

    /// `Enchantment.areCompatible` 里判断 `other` 的部分。
    pub fn excludes(&self, other: i32) -> bool {
        self.exclusive_set
            .as_ref()
            .is_some_and(|set| set.contains(&other))
    }
}

fn enchantment_defs() -> &'static Vec<EnchantmentDef> {
    static DEFS: OnceLock<Vec<EnchantmentDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        loot::ENCHANTMENTS
            .iter()
            .map(|(name, raw)| {
                parse_enchantment(name, raw).expect("生成器导出的附魔数据必须能解析")
            })
            .collect()
    })
}

fn parse_enchantment(name: &'static str, raw: &str) -> Result<EnchantmentDef> {
    let value: Value =
        serde_json::from_str(raw).map_err(|error| invalid(format!("{name}: {error}")))?;
    let map = object(&value, name)?;
    let cost = |field: &str| -> Result<(i32, i32)> {
        let value = required(map, field, name)?;
        let cost = object(value, field)?;
        Ok((
            opt_i32(cost, "base", field, 0)?,
            opt_i32(cost, "per_level_above_first", field, 0)?,
        ))
    };
    Ok(EnchantmentDef {
        name,
        weight: opt_i32(map, "weight", name, 1)?,
        // 官方的 `min_level` 缺省是 1，数据包里一般省略。
        min_level: opt_i32(map, "min_level", name, 1)?,
        max_level: opt_i32(map, "max_level", name, 1)?,
        min_cost: cost("min_cost")?,
        max_cost: cost("max_cost")?,
        supported_items: holder_set(
            required(map, "supported_items", name)?,
            Registry::Item,
            "supported_items",
        )?,
        primary_items: opt_holder_set(map, "primary_items", Registry::Item, name)?,
        exclusive_set: opt_holder_set(map, "exclusive_set", Registry::Enchantment, name)?,
        slots: super::json::string_list(required(map, "slots", name)?, "slots")?,
    })
}

/// 按注册表 ID 取附魔定义。
pub fn enchantment_def(id: i32) -> Option<&'static EnchantmentDef> {
    usize::try_from(id)
        .ok()
        .and_then(|index| enchantment_defs().get(index))
}

/// 所有附魔定义，顺序就是注册表顺序。
pub fn enchantment_defs_all() -> &'static [EnchantmentDef] {
    enchantment_defs()
}
