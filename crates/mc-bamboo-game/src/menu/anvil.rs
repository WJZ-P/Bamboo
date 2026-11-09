//! 铁砧的结果，对应 `AnvilMenu.createResult`、`setItemName` 和 `onTake`。
//!
//! 成本、结果槽的组件和取出时材料的消耗都在这里算；菜单槽的同步在 `station.rs`。

use mc_bamboo_data::{items, registries::data_component_type as kinds};
use mc_bamboo_protocol::{components::ComponentValue, item::ItemStack, text::Component};

use super::{
    enchantments,
    item_patch::{assign, resolved},
};

/// `AnvilMenu.MAX_NAME_LENGTH`，按 Java UTF-16 码元算。
pub(super) const MAX_NAME_LENGTH: usize = 50;

/// 官方 `calculateIncreasedRepairCost`。
pub(super) fn increased_repair_cost(base: i32) -> i32 {
    let next = (i64::from(base)).saturating_mul(2).saturating_add(1);
    next.min(i64::from(i32::MAX)) as i32
}

/// `StringUtil.filterText` 加 `validateName`。超长返回 `None`。
pub(super) fn validate_name(name: &str) -> Option<String> {
    // 官方按 UTF-16 的 char 过滤，Bamboo 只收解码过的 UTF-8；星界平面字符会被
    // filterText 丢掉，所以这里只保留 BMP 的合法码位。
    let filtered: String = name
        .chars()
        .filter(|&c| {
            let value = c as u32;
            (0x20..=0xffff).contains(&value) && value != 0x7f && value != 0xa7 && value != 0xfffd
        })
        .collect();
    (filtered.encode_utf16().count() <= MAX_NAME_LENGTH).then_some(filtered)
}

/// `StringUtil.isBlank`：空或全空白。
pub(super) fn is_blank(name: &str) -> bool {
    name.chars().all(char::is_whitespace)
}

/// `ItemStack.getHoverName().getString()`。
///
/// Bamboo 没有客户端语言表，物品名是翻译组件；只有 `custom_name` 能取出纯文本。
/// 这和官方在改名前后比较输入字符串的结果一致：没改过名时不会等于任何玩家输入。
fn hover_name(stack: &ItemStack) -> Option<String> {
    match resolved(stack, kinds::CUSTOM_NAME) {
        Some(ComponentValue::CustomName(component)) => component_string(&component),
        _ => None,
    }
}

/// 文本组件的纯文本。Bamboo 的组件只保存 NBT 标签，`Tag::String` 是字面文本。
fn component_string(component: &Component) -> Option<String> {
    match component.tag() {
        mc_bamboo_nbt::Tag::String(value) => Some(value.clone()),
        mc_bamboo_nbt::Tag::Compound(entries) => entries
            .iter()
            .find(|(key, _)| key == "text")
            .and_then(|(_, value)| match value {
                mc_bamboo_nbt::Tag::String(value) => Some(value.clone()),
                _ => None,
            }),
        _ => None,
    }
}

/// 官方 `PatchedDataComponentMap.has`：补丁和默认组件合起来有这一项。
fn has(stack: &ItemStack, kind: i32) -> bool {
    resolved(stack, kind).is_some()
}

/// `EnchantmentHelper.canStoreEnchantments`。
pub(super) fn can_store_enchantments(stack: &ItemStack) -> bool {
    let kind = component(stack);
    has(stack, kind)
}

/// 附魔书用 `stored_enchantments`，别的物品用 `enchantments`。
pub(super) fn component(stack: &ItemStack) -> i32 {
    if stack.item == items::ENCHANTED_BOOK {
        kinds::STORED_ENCHANTMENTS
    } else {
        kinds::ENCHANTMENTS
    }
}

/// `EnchantmentHelper.getEnchantmentsForCrafting`：组件不在时是空表。
pub(super) fn levels(stack: &ItemStack) -> Vec<(i32, i32)> {
    match resolved(stack, component(stack)) {
        Some(ComponentValue::Enchantments(v) | ComponentValue::StoredEnchantments(v)) => v.clone(),
        _ => Vec::new(),
    }
}

/// `EnchantmentHelper.setEnchantments`。
pub(super) fn set_levels(stack: &mut ItemStack, list: &[(i32, i32)]) {
    let kind = component(stack);
    let value = if kind == kinds::STORED_ENCHANTMENTS {
        ComponentValue::StoredEnchantments(list.to_vec())
    } else {
        ComponentValue::Enchantments(list.to_vec())
    };
    assign(stack, kind, Some(value));
}

fn level_of(list: &[(i32, i32)], id: i32) -> i32 {
    list.iter()
        .find(|(stored, _)| *stored == id)
        .map(|(_, level)| *level)
        .unwrap_or(0)
}

/// `ItemEnchantments.Mutable.set`：等级夹在 0..=255，重复时留第一次出现的位置。
fn upgrade(list: &mut Vec<(i32, i32)>, id: i32, level: i32) {
    let level = level.clamp(0, 255);
    if level <= 0 {
        return;
    }
    if let Some((_, stored)) = list.iter_mut().find(|(stored, _)| *stored == id) {
        *stored = (*stored).max(level);
    } else {
        list.push((id, level));
    }
}

/// `ItemStack.isDamageableItem`。
pub(super) fn is_damageable(stack: &ItemStack) -> bool {
    has(stack, kinds::MAX_DAMAGE) && !has(stack, kinds::UNBREAKABLE) && has(stack, kinds::DAMAGE)
}

/// `ItemStack.isValidRepairItem`。`repairable` 组件里的标签或 ID 列表。
pub(super) fn is_repair_item(target: &ItemStack, material: &ItemStack) -> bool {
    let Some(ComponentValue::Repairable(set)) = resolved(target, kinds::REPAIRABLE) else {
        return false;
    };
    match set {
        mc_bamboo_protocol::components::cursor::HolderSet::Tag(name) => {
            mc_bamboo_data::item_tag(&name).is_some_and(|ids| ids.contains(&material.item))
        }
        mc_bamboo_protocol::components::cursor::HolderSet::Ids(ids) => ids.contains(&material.item),
    }
}

pub(super) fn max_damage(stack: &ItemStack) -> i32 {
    match resolved(stack, kinds::MAX_DAMAGE) {
        Some(ComponentValue::MaxDamage(value)) => value,
        _ => 0,
    }
}

pub(super) fn damage(stack: &ItemStack) -> i32 {
    let value = match resolved(stack, kinds::DAMAGE) {
        Some(ComponentValue::Damage(value)) => value,
        _ => 0,
    };
    value.clamp(0, max_damage(stack))
}

pub(super) fn set_damage(stack: &mut ItemStack, value: i32) {
    let value = value.clamp(0, max_damage(stack));
    assign(stack, kinds::DAMAGE, Some(ComponentValue::Damage(value)));
}

pub(super) fn repair_cost(stack: &ItemStack) -> i32 {
    match resolved(stack, kinds::REPAIR_COST) {
        Some(ComponentValue::RepairCost(value)) => value,
        _ => 0,
    }
}

/// 一次 `createResult` 的全部产物。
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Outcome {
    /// `cost` DataSlot。
    pub cost: i32,
    /// `repairItemCountCost`。
    pub repair_item_count_cost: i32,
    /// `onlyRenaming`。
    pub only_renaming: bool,
}

/// `AnvilMenu.mayPickup`：创造或等级够，而且成本大于零。
pub(super) fn may_pickup(cost: i32, creative: bool, experience_level: i32) -> bool {
    (creative || experience_level >= cost) && cost > 0
}

/// `AnvilMenu.createResult`。
///
/// `name` 是 `setItemName` 之后的名字（还没有就是 `None`）；`creative` 是 `hasInfiniteMaterials`。
/// 返回时 `slots[2]` 已经写好结果，`cost` 也已经更新。
pub(super) fn create_result(
    input: &ItemStack,
    additional: &ItemStack,
    name: Option<&str>,
    creative: bool,
    result: &mut ItemStack,
) -> Outcome {
    let mut outcome = Outcome {
        cost: 1,
        ..Default::default()
    };
    let mut cost = 0i32;
    let mut base = 0i64;
    let mut rename = 0i32;

    if input.is_empty() || !can_store_enchantments(input) {
        *result = ItemStack::empty();
        outcome.cost = 0;
        return outcome;
    }

    let mut output = input.clone();
    let mut merged = levels(input);
    base += i64::from(repair_cost(input)) + i64::from(repair_cost(additional));
    outcome.repair_item_count_cost = 0;

    if !additional.is_empty() {
        let is_book = has(additional, kinds::STORED_ENCHANTMENTS);
        if is_damageable(&output) && is_repair_item(input, additional) {
            let mut repair = damage(&output).min(max_damage(&output) / 4);
            if repair <= 0 {
                *result = ItemStack::empty();
                outcome.cost = 0;
                return outcome;
            }
            let mut used = 0;
            while repair > 0 && used < additional.count {
                let next = damage(&output) - repair;
                set_damage(&mut output, next);
                cost += 1;
                repair = damage(&output).min(max_damage(&output) / 4);
                used += 1;
            }
            outcome.repair_item_count_cost = used;
        } else {
            if !is_book && !(output.item == additional.item && is_damageable(&output)) {
                *result = ItemStack::empty();
                outcome.cost = 0;
                return outcome;
            }
            if is_damageable(&output) && !is_book {
                let remaining1 = max_damage(input) - damage(input);
                let remaining2 = max_damage(additional) - damage(additional);
                let bonus = max_damage(&output) * 12 / 100;
                let remaining = remaining1 + remaining2 + bonus;
                let next_damage = (max_damage(&output) - remaining).max(0);
                if next_damage < damage(&output) {
                    set_damage(&mut output, next_damage);
                    cost += 2;
                }
            }

            let mut any_incompatible = false;
            let mut any_enchantment = false;
            for (id, incoming_level) in levels(additional) {
                let mut level = if level_of(&merged, id) == incoming_level {
                    incoming_level + 1
                } else {
                    incoming_level.max(level_of(&merged, id))
                };
                let definition = enchantments::get(id);
                let mut allowed = enchantments::can_enchant(id, input.item);
                if creative || input.item == items::ENCHANTED_BOOK {
                    allowed = true;
                }
                for &(stored, _) in &merged {
                    if stored != id && !enchantments::compatible(id, stored) {
                        allowed = false;
                        cost += 1;
                    }
                }
                if !allowed {
                    any_incompatible = true;
                    continue;
                }
                any_enchantment = true;
                if level > definition.max_level {
                    level = definition.max_level;
                }
                upgrade(&mut merged, id, level);
                let mut anvil_cost = definition.anvil_cost;
                if is_book {
                    anvil_cost = anvil_cost.div_euclid(2).max(1);
                }
                cost += anvil_cost * level;
                if input.count > 1 {
                    cost = 40;
                }
            }
            if any_incompatible && !any_enchantment {
                *result = ItemStack::empty();
                outcome.cost = 0;
                return outcome;
            }
        }
    }

    match name {
        None if has(input, kinds::CUSTOM_NAME) => {
            rename += 1;
            cost += rename;
            assign(&mut output, kinds::CUSTOM_NAME, None);
        }
        Some(name) if !is_blank(name) => {
            if hover_name(input).as_deref() != Some(name) {
                rename += 1;
                cost += rename;
                assign(
                    &mut output,
                    kinds::CUSTOM_NAME,
                    Some(ComponentValue::CustomName(Component::text(name))),
                );
            }
        }
        Some(_) if has(input, kinds::CUSTOM_NAME) => {
            rename += 1;
            cost += rename;
            assign(&mut output, kinds::CUSTOM_NAME, None);
        }
        _ => {}
    }

    outcome.cost = if cost > 0 {
        (base + i64::from(cost)).clamp(0, i64::from(i32::MAX)) as i32
    } else {
        0
    };
    if cost <= 0 {
        output = ItemStack::empty();
    }
    if rename == cost && rename > 0 {
        if outcome.cost >= 40 {
            outcome.cost = 39;
        }
        outcome.only_renaming = true;
    }
    if outcome.cost >= 40 && !creative {
        output = ItemStack::empty();
    }

    if !output.is_empty() {
        let mut next = repair_cost(&output).max(repair_cost(additional));
        if !(rename == cost && rename > 0) {
            next = increased_repair_cost(next);
        }
        assign(
            &mut output,
            kinds::REPAIR_COST,
            Some(ComponentValue::RepairCost(next)),
        );
        set_levels(&mut output, &merged);
    }
    *result = output;
    outcome
}

/// `setItemName` 里对结果槽先做的那一步（不重算成本）。
pub(super) fn apply_name_to_result(result: &mut ItemStack, name: &str) {
    if result.is_empty() {
        return;
    }
    if is_blank(name) {
        assign(result, kinds::CUSTOM_NAME, None);
    } else {
        assign(
            result,
            kinds::CUSTOM_NAME,
            Some(ComponentValue::CustomName(Component::text(name))),
        );
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::components::ComponentValue;

    use super::*;

    fn stack(item: i32, count: i32) -> ItemStack {
        ItemStack::new(item, count)
    }

    fn with_enchantment(item: i32, id: i32, level: i32) -> ItemStack {
        let mut result = stack(item, 1);
        assign(
            &mut result,
            kinds::ENCHANTMENTS,
            Some(ComponentValue::Enchantments(vec![(id, level)])),
        );
        result
    }

    fn damaged(item: i32, damage: i32) -> ItemStack {
        let mut result = stack(item, 1);
        assign(
            &mut result,
            kinds::DAMAGE,
            Some(ComponentValue::Damage(damage)),
        );
        result
    }

    #[test]
    fn empty_input_has_no_result() {
        let mut result = ItemStack::empty();
        let outcome = create_result(
            &ItemStack::empty(),
            &ItemStack::empty(),
            None,
            false,
            &mut result,
        );
        assert!(result.is_empty());
        assert_eq!(outcome.cost, 0);
    }

    #[test]
    fn renaming_a_sword_costs_one_and_writes_the_name() {
        let sword = stack(items::DIAMOND_SWORD, 1);
        let mut result = ItemStack::empty();
        let outcome = create_result(
            &sword,
            &ItemStack::empty(),
            Some("Excalibur"),
            false,
            &mut result,
        );
        assert_eq!(outcome.cost, 1);
        assert!(outcome.only_renaming);
        assert_eq!(repair_cost(&result), 0, "纯改名不涨 repair_cost");
        assert!(matches!(
            resolved(&result, kinds::CUSTOM_NAME),
            Some(ComponentValue::CustomName(_))
        ));
    }

    #[test]
    fn a_damaged_sword_and_a_diamond_repair_it() {
        let sword = damaged(items::DIAMOND_SWORD, 100);
        let diamond = stack(items::DIAMOND, 3);
        let mut result = ItemStack::empty();
        let outcome = create_result(&sword, &diamond, None, false, &mut result);
        assert_eq!(outcome.repair_item_count_cost, 1);
        assert_eq!(damage(&result), 0);
        assert_eq!(outcome.cost, 1);
    }

    #[test]
    fn merging_two_swords_adds_durability() {
        let left = damaged(items::DIAMOND_SWORD, 100);
        let right = damaged(items::DIAMOND_SWORD, 100);
        let mut result = ItemStack::empty();
        let outcome = create_result(&left, &right, None, false, &mut result);
        assert!(damage(&result) < 100);
        assert_eq!(outcome.cost, 2);
        assert_eq!(repair_cost(&result), 1);
    }

    #[test]
    fn an_enchanted_book_merges_its_level() {
        let sharpness = enchantments::get(33);
        assert_eq!(sharpness.max_level, 5);
        let sword = with_enchantment(items::DIAMOND_SWORD, 33, 1);
        let mut book = stack(items::ENCHANTED_BOOK, 1);
        assign(
            &mut book,
            kinds::STORED_ENCHANTMENTS,
            Some(ComponentValue::StoredEnchantments(vec![(33, 1)])),
        );
        let mut result = ItemStack::empty();
        create_result(&sword, &book, None, false, &mut result);
        assert_eq!(levels(&result), vec![(33, 2)]);
    }

    #[test]
    fn a_name_over_fifty_utf16_units_is_rejected() {
        assert_eq!(validate_name("ok"), Some("ok".to_owned()));
        assert!(validate_name(&"x".repeat(51)).is_none());
        assert_eq!(validate_name("a\u{7f}b"), Some("ab".to_owned()));
    }
}
