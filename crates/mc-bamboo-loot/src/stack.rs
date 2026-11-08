//! 战利品表产出的一件物品，对应官方的 `ItemStack`。
//!
//! 官方的物品组件是编解码器，另一组还在做。这里只记下「要往哪个组件写什么」，
//! 组件值原样保存成 JSON，等组件编解码器做好后再转成真正的字节。
//! 判断附魔等级时只读 `minecraft:enchantments` 这一个组件的结构，不涉及编码。

use std::collections::BTreeMap;

use mc_bamboo_data::{items, Item};
use serde_json::{Map, Value};

use crate::names;

/// 组件补丁：组件名 → 官方 codec 的 JSON 表示。只保存被函数显式改过的组件。
pub type Components = BTreeMap<String, Value>;

/// 战利品函数会读写的那几个组件名，集中放在这里避免拼错。
pub const ENCHANTMENTS: &str = "minecraft:enchantments";
pub const STORED_ENCHANTMENTS: &str = "minecraft:stored_enchantments";
pub const DAMAGE: &str = "minecraft:damage";
pub const BLOCK_STATE: &str = "minecraft:block_state";
pub const POTION_CONTENTS: &str = "minecraft:potion_contents";
pub const SUSPICIOUS_STEW_EFFECTS: &str = "minecraft:suspicious_stew_effects";
pub const ADDITIONAL_TRADE_COST: &str = "minecraft:additional_trade_cost";
pub const OMINOUS_BOTTLE_AMPLIFIER: &str = "minecraft:ominous_bottle_amplifier";
pub const INSTRUMENT: &str = "minecraft:instrument";
pub const CUSTOM_NAME: &str = "minecraft:custom_name";
pub const ITEM_NAME: &str = "minecraft:item_name";

/// 附魔等级查询。真实现读物品组件，测试里可以用固定表，不必碰组件编码。
pub trait EnchantmentLevels {
    /// 这件物品上某个附魔的等级，没有就是 0。参数是附魔注册表 ID。
    fn enchantment_level(&self, enchantment: i32) -> i32;
}

/// 一件战利品。`item` 是物品注册表 ID，`count` 和官方一样允许超出最大堆叠数，
/// 由 `LootTable` 最后的拆分器决定怎么拆。
#[derive(Debug, Clone, PartialEq)]
pub struct LootStack {
    pub item: i32,
    pub count: i32,
    pub components: Components,
}

impl LootStack {
    pub fn new(item: i32, count: i32) -> Self {
        Self {
            item,
            count,
            components: Components::new(),
        }
    }

    pub fn empty() -> Self {
        Self::new(items::AIR, 0)
    }

    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item == items::AIR
    }

    pub fn item(&self) -> &'static Item {
        &items::ITEMS[self.item as usize]
    }

    pub fn name(&self) -> &'static str {
        self.item().name
    }

    /// `ItemStack.getMaxStackSize`。
    pub fn max_stack_size(&self) -> i32 {
        self.item().max_stack_size
    }

    /// `ItemStack.copyWithCount`：数量不大于 0 时得到空栈。
    pub fn copy_with_count(&self, count: i32) -> Self {
        if count <= 0 {
            Self::empty()
        } else {
            let mut copy = self.clone();
            copy.count = count;
            copy
        }
    }

    /// `ItemStack.copy`。
    pub fn copy(&self) -> Self {
        self.clone()
    }

    /// 按最大堆叠数拆成若干份，对应 `createStackSplitter` 的循环。
    pub fn split_by_max_stack(&self) -> Vec<Self> {
        let max = self.max_stack_size();
        if self.count < max {
            return vec![self.copy()];
        }
        let mut parts = Vec::new();
        let mut left = self.count;
        while left > 0 {
            let part = self.copy_with_count(max.min(left));
            left -= part.count;
            parts.push(part);
        }
        parts
    }

    pub fn component(&self, name: &str) -> Option<&Value> {
        self.components.get(name)
    }

    pub fn set_component(&mut self, name: &str, value: Value) {
        self.components.insert(name.to_owned(), value);
    }

    pub fn remove_component(&mut self, name: &str) {
        self.components.remove(name);
    }

    /// `ItemStack.setCount`：官方原样赋值，数量可以是 0 或负数，`isEmpty` 再判断。
    pub fn set_count(&mut self, count: i32) {
        self.count = count;
    }

    /// `ItemStack.grow`。
    pub fn grow(&mut self, amount: i32) {
        self.count = self.count.wrapping_add(amount);
    }

    /// `ItemStack.limitSize`：空栈不动，超过上限才截断。
    pub fn limit_size(&mut self, max: i32) {
        if !self.is_empty() && self.count > max {
            self.count = max;
        }
    }

    /// `ItemStack.split`：先截出 `amount` 份，再把这边的数量减掉。
    pub fn split(&mut self, amount: i32) -> Self {
        let real_amount = amount.min(self.count);
        let result = self.copy_with_count(real_amount);
        self.grow(-real_amount);
        result
    }

    /// `ItemStack.transmuteCopy`：空栈还是空栈，否则保留数量和组件换物品。
    pub fn transmute_copy(&self, item: i32) -> Self {
        if self.is_empty() {
            return Self::empty();
        }
        let mut copy = self.clone();
        copy.item = item;
        copy
    }

    /// 附魔书的附魔写在 `stored_enchantments`，其他物品写在 `enchantments`。
    pub fn enchantment_component(&self) -> &'static str {
        if self.item == items::ENCHANTED_BOOK {
            STORED_ENCHANTMENTS
        } else {
            ENCHANTMENTS
        }
    }

    /// 组件里的附魔等级表，组件缺失或格式不对时当成空。
    pub fn enchantment_levels(&self, component: &str) -> BTreeMap<String, i32> {
        self.components
            .get(component)
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(name, level)| {
                        let level = i32::try_from(level.as_i64()?).ok()?;
                        Some((name.clone(), level))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 写回附魔等级表，空表等同于默认组件，直接删掉。
    pub fn set_enchantment_levels(&mut self, component: &str, levels: BTreeMap<String, i32>) {
        if levels.is_empty() {
            self.remove_component(component);
            return;
        }
        let map = levels
            .into_iter()
            .map(|(name, level)| (name, Value::from(level)))
            .collect::<Map<_, _>>();
        self.set_component(component, Value::Object(map));
    }

    /// `ItemStack.enchant`：已有等级取较大者，最后截到 255。
    pub fn enchant(&mut self, enchantment: i32, level: i32) {
        let Some(name) = names::enchantment_name(enchantment) else {
            return;
        };
        let component = self.enchantment_component();
        let mut levels = self.enchantment_levels(component);
        let current = levels.get(name).copied().unwrap_or(0);
        let level = current.max(level).min(255);
        if level <= 0 {
            levels.remove(name);
        } else {
            levels.insert(name.to_owned(), level);
        }
        self.set_enchantment_levels(component, levels);
    }

    /// `ItemStack.isDamageableItem`：战利品里的物品不会带 `unbreakable`，
    /// 所以有 `max_damage` 就按可损坏处理。
    pub fn is_damageable(&self) -> bool {
        self.item().max_damage.is_some()
    }

    /// `ItemStack.getMaxDamage`。
    pub fn max_damage(&self) -> i32 {
        self.item().max_damage.unwrap_or(0)
    }

    /// `ItemStack.getDamageValue`：缺省 0，并截到 [0, max_damage]。
    pub fn damage_value(&self) -> i32 {
        self.components
            .get(DAMAGE)
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(0)
            .clamp(0, self.max_damage())
    }

    /// `ItemStack.setDamageValue`：截到 [0, max_damage]，0 是默认值不写进补丁。
    pub fn set_damage_value(&mut self, value: i32) {
        let value = value.clamp(0, self.max_damage());
        if value == 0 {
            self.remove_component(DAMAGE);
        } else {
            self.set_component(DAMAGE, Value::from(value));
        }
    }

    /// `EnchantmentHelper.getItemEnchantmentLevel`：只读 `minecraft:enchantments`，
    /// 附魔书也一样。名字从注册表 ID 反查。
    pub fn item_enchantment_level(&self, enchantment: i32) -> i32 {
        let Some(name) = names::enchantment_name(enchantment) else {
            return 0;
        };
        self.enchantment_levels(ENCHANTMENTS)
            .get(name)
            .copied()
            .unwrap_or(0)
    }
}

impl EnchantmentLevels for LootStack {
    fn enchantment_level(&self, enchantment: i32) -> i32 {
        self.item_enchantment_level(enchantment)
    }
}

/// 没有附魔的固定实现，测试和空手时用。
pub struct NoEnchantments;

impl EnchantmentLevels for NoEnchantments {
    fn enchantment_level(&self, _enchantment: i32) -> i32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting_uses_the_max_stack_size() {
        let stack = LootStack::new(items::STONE, 130);
        let parts = stack.split_by_max_stack();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].count, 64);
        assert_eq!(parts[1].count, 64);
        assert_eq!(parts[2].count, 2);
    }

    #[test]
    fn small_counts_are_not_split() {
        let stack = LootStack::new(items::STONE, 64);
        assert_eq!(stack.split_by_max_stack(), vec![stack]);
    }

    #[test]
    fn enchantment_levels_come_from_the_component() {
        let mut stack = LootStack::new(items::DIAMOND_PICKAXE, 1);
        stack.set_component(
            "minecraft:enchantments",
            serde_json::json!({"minecraft:fortune": 3}),
        );
        let fortune = names::enchantment_id("minecraft:fortune").unwrap();
        assert_eq!(stack.enchantment_level(fortune), 3);
        let looting = names::enchantment_id("minecraft:looting").unwrap();
        assert_eq!(stack.enchantment_level(looting), 0);
    }
}
