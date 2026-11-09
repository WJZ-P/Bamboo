//! 砂轮的结果和取出时的经验，对应 `GrindstoneMenu` 和 `ExperienceOrb.award`。
//!
//! 诅咒留下，其余附魔去掉。两件同样的物品按官方公式合并耐久。经验先把两边非诅咒附魔的
//! `min_cost` 加起来，再从调用方给的随机源取数。经验球不生成，但每一次 `nextInt` 都照取。
//!
//! Grindstone results and the experience awarded on take, matching `GrindstoneMenu` and
//! `ExperienceOrb.award`.
//!
//! Curses stay and every other enchantment is removed. Two of the same item merge durability with
//! the vanilla formula. Experience sums `min_cost` of the non-curse enchantments, then draws from
//! the caller's random source. No experience orb is spawned, but every `nextInt` is still drawn.

use std::sync::OnceLock;

use mc_bamboo_blocks::RandomSource;
use mc_bamboo_data::{
    items, loot,
    registries::{self, data_component_type as kinds},
    tags,
};
use mc_bamboo_protocol::{
    components::{ComponentValue, PatchEntry},
    item::ItemStack,
};

use super::item_patch::{assign, patch_of, resolved};

/// `ExperienceOrb.getExperienceValue` 的分段上限。
///
/// The thresholds in `ExperienceOrb.getExperienceValue`.
const EXPERIENCE_VALUES: [i32; 10] = [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3];

const ENCHANTMENT_REGISTRY: &str = "minecraft:enchantment";
const CURSE_TAG: &str = "minecraft:curse";

/// 官方结果槽的 `mayPlace`：能掉耐久，或者身上有附魔。
///
/// The result-slot `mayPlace` is false. Input slots accept a damageable item or any enchantment.
pub(super) fn may_place(stack: &ItemStack) -> bool {
    is_damageable(stack) || has_any_enchantments(stack)
}

/// 官方 `computeResult`。
///
/// Vanilla `computeResult`.
pub(super) fn compute(input: &ItemStack, additional: &ItemStack) -> ItemStack {
    if input.is_empty() && additional.is_empty() {
        return ItemStack::empty();
    }
    if input.count > 1 || additional.count > 1 {
        return ItemStack::empty();
    }
    if input.is_empty() || additional.is_empty() {
        let item = if input.is_empty() { additional } else { input };
        if !has_any_enchantments(item) {
            return ItemStack::empty();
        }
        return remove_non_curses(&mut item.clone());
    }
    merge(input, additional)
}

/// 取出之前两边非诅咒附魔的 `min_cost` 之和。随机数还没取。
///
/// The sum of `min_cost` for non-curse enchantments on both inputs, before any random draw.
pub(super) fn experience_sum(input: &ItemStack, additional: &ItemStack) -> i32 {
    experience_of(input) + experience_of(additional)
}

/// 官方 `getExperienceAmount` 里的 `nextInt`，接着 `ExperienceOrb.award` 的每一次 `nextInt(40)`。
///
/// 没有经验球。`amount` 不大于 0 时官方一次都不取。
///
/// The `nextInt` inside `getExperienceAmount`, then one `nextInt(40)` per split inside
/// `ExperienceOrb.award`. No orb is spawned. A non-positive `amount` draws nothing.
pub(super) fn award(random: &impl RandomSource, amount: i32) {
    if amount <= 0 {
        return;
    }
    let half = ((amount as f64) / 2.0).ceil() as i32;
    let mut left = half + random.next_int(half);
    while left > 0 {
        left -= experience_value(left);
        // `tryMergeToExisting` 先取这个数，再发现附近没有经验球。
        // `tryMergeToExisting` draws this before it finds no orb nearby.
        random.next_int(40);
    }
}

fn merge(input: &ItemStack, additional: &ItemStack) -> ItemStack {
    if input.item != additional.item {
        return ItemStack::empty();
    }
    let durability = max_damage(input).max(max_damage(additional));
    let remaining1 = max_damage(input) - damage_value(input);
    let remaining2 = max_damage(additional) - damage_value(additional);
    // 官方是 `int` 乘法，先溢出再除。真实耐久不会溢出。
    // Vanilla multiplies in `int`, wrapping before the division. Real durability does not wrap.
    let bonus = durability.wrapping_mul(5) / 100;
    let remaining = remaining1.wrapping_add(remaining2).wrapping_add(bonus);
    if !is_damageable(input) && (input.max_stack_size() < 2 || !input.matches(additional)) {
        return ItemStack::empty();
    }
    let count = if is_damageable(input) { 1 } else { 2 };
    let mut result = input.clone();
    result.count = count;
    if is_damageable(&result) {
        assign(
            &mut result,
            kinds::MAX_DAMAGE,
            Some(ComponentValue::MaxDamage(durability)),
        );
        set_damage(&mut result, durability.wrapping_sub(remaining).max(0));
    }
    merge_enchants(&mut result, additional);
    remove_non_curses(&mut result)
}

fn merge_enchants(target: &mut ItemStack, source: &ItemStack) {
    let incoming = crafting_levels(source);
    update_enchantments(target, |levels| {
        for (id, level) in incoming {
            if is_curse(id) && level_of(levels, id) != 0 {
                continue;
            }
            upgrade(levels, id, level);
        }
    });
}

fn remove_non_curses(stack: &mut ItemStack) -> ItemStack {
    let remaining = update_enchantments(stack, |levels| {
        levels.retain(|(id, _)| is_curse(*id));
    });
    if stack.item == items::ENCHANTED_BOOK && remaining.is_empty() {
        *stack = transmute(stack, items::BOOK);
    }
    let mut repair_cost = 0;
    for _ in &remaining {
        repair_cost = increased_repair_cost(repair_cost);
    }
    assign(
        stack,
        kinds::REPAIR_COST,
        Some(ComponentValue::RepairCost(repair_cost)),
    );
    stack.clone()
}

/// 官方 `EnchantmentHelper.updateEnchantments`。组件不在时什么都不写，返回空表。
///
/// Vanilla `EnchantmentHelper.updateEnchantments`. A missing component is left untouched and the
/// returned list is empty.
fn update_enchantments(
    stack: &mut ItemStack,
    edit: impl FnOnce(&mut Vec<(i32, i32)>),
) -> Vec<(i32, i32)> {
    let kind = crafting_component(stack);
    let Some(current) = resolved(stack, kind) else {
        return Vec::new();
    };
    let mut levels = match current {
        ComponentValue::Enchantments(levels) | ComponentValue::StoredEnchantments(levels) => levels,
        _ => return Vec::new(),
    };
    edit(&mut levels);
    let value = if kind == kinds::STORED_ENCHANTMENTS {
        ComponentValue::StoredEnchantments(levels.clone())
    } else {
        ComponentValue::Enchantments(levels.clone())
    };
    assign(stack, kind, Some(value));
    levels
}

fn transmute(stack: &ItemStack, item: i32) -> ItemStack {
    let patch = patch_of(stack);
    let mut result = ItemStack::new(item, stack.count);
    for (kind, entry) in patch.entries() {
        match entry {
            PatchEntry::Removed => assign(&mut result, *kind, None),
            PatchEntry::Set(value) => assign(&mut result, *kind, Some(value.clone())),
        }
    }
    result
}

fn crafting_component(stack: &ItemStack) -> i32 {
    if stack.item == items::ENCHANTED_BOOK {
        kinds::STORED_ENCHANTMENTS
    } else {
        kinds::ENCHANTMENTS
    }
}

fn crafting_levels(stack: &ItemStack) -> Vec<(i32, i32)> {
    match resolved(stack, crafting_component(stack)) {
        Some(ComponentValue::Enchantments(levels) | ComponentValue::StoredEnchantments(levels)) => {
            levels
        }
        _ => Vec::new(),
    }
}

fn has_any_enchantments(stack: &ItemStack) -> bool {
    list_nonempty(stack, kinds::ENCHANTMENTS) || list_nonempty(stack, kinds::STORED_ENCHANTMENTS)
}

fn list_nonempty(stack: &ItemStack, kind: i32) -> bool {
    match resolved(stack, kind) {
        Some(ComponentValue::Enchantments(levels) | ComponentValue::StoredEnchantments(levels)) => {
            !levels.is_empty()
        }
        _ => false,
    }
}

fn upgrade(levels: &mut Vec<(i32, i32)>, id: i32, level: i32) {
    if level <= 0 {
        return;
    }
    let level = level.min(255);
    if let Some((_, stored)) = levels.iter_mut().find(|(stored, _)| *stored == id) {
        *stored = (*stored).max(level).min(255);
    } else {
        levels.push((id, level));
    }
}

fn level_of(levels: &[(i32, i32)], id: i32) -> i32 {
    levels
        .iter()
        .find(|(stored, _)| *stored == id)
        .map(|(_, level)| *level)
        .unwrap_or(0)
}

fn is_damageable(stack: &ItemStack) -> bool {
    resolved(stack, kinds::MAX_DAMAGE).is_some()
        && resolved(stack, kinds::UNBREAKABLE).is_none()
        && resolved(stack, kinds::DAMAGE).is_some()
}

fn max_damage(stack: &ItemStack) -> i32 {
    match resolved(stack, kinds::MAX_DAMAGE) {
        Some(ComponentValue::MaxDamage(value)) => value,
        _ => 0,
    }
}

fn damage_value(stack: &ItemStack) -> i32 {
    let value = match resolved(stack, kinds::DAMAGE) {
        Some(ComponentValue::Damage(value)) => value,
        _ => 0,
    };
    value.clamp(0, max_damage(stack))
}

fn set_damage(stack: &mut ItemStack, value: i32) {
    let value = value.clamp(0, max_damage(stack));
    assign(stack, kinds::DAMAGE, Some(ComponentValue::Damage(value)));
}

/// 官方 `AnvilMenu.calculateIncreasedRepairCost`。
///
/// Vanilla `AnvilMenu.calculateIncreasedRepairCost`.
fn increased_repair_cost(base: i32) -> i32 {
    let next = (i64::from(base)).saturating_mul(2).saturating_add(1);
    next.min(i64::from(i32::MAX)) as i32
}

fn experience_of(stack: &ItemStack) -> i32 {
    crafting_levels(stack)
        .into_iter()
        .filter(|(id, _)| !is_curse(*id))
        .map(|(id, level)| enchantment_cost(id, level))
        .sum()
}

fn enchantment_cost(id: i32, level: i32) -> i32 {
    let (base, per_level) = costs()
        .get(usize::try_from(id).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or((0, 0));
    base + per_level * (level - 1)
}

fn experience_value(amount: i32) -> i32 {
    for limit in EXPERIENCE_VALUES {
        if amount >= limit {
            return limit;
        }
    }
    1
}

fn is_curse(id: i32) -> bool {
    curse_ids().contains(&id)
}

fn curse_ids() -> &'static [i32] {
    tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == ENCHANTMENT_REGISTRY)
        .and_then(|group| group.tags.iter().find(|tag| tag.name == CURSE_TAG))
        .map(|tag| tag.ids)
        .unwrap_or(&[])
}

fn costs() -> &'static [(i32, i32)] {
    static COSTS: OnceLock<Vec<(i32, i32)>> = OnceLock::new();
    COSTS.get_or_init(|| {
        let names = registries::SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == ENCHANTMENT_REGISTRY)
            .map(|registry| registry.entries)
            .unwrap_or(&[]);
        let mut costs = vec![(0, 0); names.len()];
        for (name, json) in loot::ENCHANTMENTS {
            let Some(id) = names.iter().position(|entry| entry == name) else {
                continue;
            };
            costs[id] = min_cost(json);
        }
        costs
    })
}

/// 从附魔 JSON 里读 `min_cost`，不读前面的 `max_cost`。
///
/// Read `min_cost` from the enchantment JSON, not the earlier `max_cost`.
fn min_cost(json: &str) -> (i32, i32) {
    let Some(start) = json.find("\"min_cost\"") else {
        return (0, 0);
    };
    let rest = &json[start..];
    (
        field_int(rest, "\"base\""),
        field_int(rest, "\"per_level_above_first\""),
    )
}

fn field_int(text: &str, key: &str) -> i32 {
    let Some(at) = text.find(key) else {
        return 0;
    };
    let after = &text[at + key.len()..];
    let number: String = after
        .chars()
        .skip_while(|c| !c.is_ascii_digit() && *c != '-')
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    number.parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::LegacyRandom;
    use mc_bamboo_data::items;
    use mc_bamboo_protocol::components::ComponentValue;

    use super::*;

    fn plain(item: i32) -> ItemStack {
        ItemStack::new(item, 1)
    }

    #[test]
    fn two_sticks_become_a_stack_of_two_with_zero_repair_cost() {
        let result = compute(&plain(items::STICK), &plain(items::STICK));
        assert_eq!(result.item, items::STICK);
        assert_eq!(result.count, 2);
        assert_eq!(
            resolved(&result, kinds::REPAIR_COST),
            Some(ComponentValue::RepairCost(0))
        );
    }

    #[test]
    fn two_undamaged_swords_stay_one_undamaged_sword() {
        let result = compute(&plain(items::DIAMOND_SWORD), &plain(items::DIAMOND_SWORD));
        assert_eq!(result.item, items::DIAMOND_SWORD);
        assert_eq!(result.count, 1);
        assert_eq!(damage_value(&result), 0);
        assert!(may_place(&plain(items::DIAMOND_SWORD)));
        assert!(!may_place(&plain(items::STICK)));
    }

    #[test]
    fn a_stack_or_two_different_items_make_nothing() {
        let mut stacked = plain(items::STICK);
        stacked.count = 2;
        assert!(compute(&stacked, &ItemStack::empty()).is_empty());
        assert!(compute(&plain(items::STICK), &plain(items::DIAMOND_SWORD)).is_empty());
    }

    #[test]
    fn binding_curse_min_cost_comes_from_the_exported_json() {
        assert_eq!(enchantment_cost(2, 1), 25);
        assert!(is_curse(2));
    }

    #[test]
    fn a_positive_award_draws_the_half_and_one_merge_attempt() {
        let random = LegacyRandom::new(3);
        award(&random, 1);
        let expected = LegacyRandom::new(3);
        let xp = 1 + expected.next_int(1);
        assert_eq!(xp, 1);
        expected.next_int(40);
        assert_eq!(random.seed(), expected.seed());
        let untouched = LegacyRandom::new(3);
        award(&untouched, 0);
        assert_eq!(untouched.seed(), LegacyRandom::new(3).seed());
    }
}
