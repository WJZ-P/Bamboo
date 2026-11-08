//! 工作站要用的查询：燃料、制作剩余物、`RecipePropertySet` 对应的物品集合、
//! 装备槽和「同物品同组件」的比较。
//!
//! 官方的 `RecipePropertySet` 在 `RecipeManager.finalizeRecipeLoading` 里由各类配方
//! 的原料展开而成，只比对物品 ID，不看组件。这里按同样的方式从导出表算一次，
//! 之后用二分查找。`CookingFuel`、`BrewingFuel` 里的可解析数值按官方
//! `ContextIntProviders`、`ContextFloatProviders` 的表求值。

use std::sync::OnceLock;

use mc_bamboo_data::{
    CookingKind, ItemTag, Recipe, RecipeKind, RecipeType,
    recipes::RECIPES,
    registries::data_component_type as kinds,
    tags::{TAG_GROUPS, item_tags},
};
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, ComponentValue,
        value::{ResolvableFloat, ResolvableInt},
    },
    item::ItemStack,
};

/// 官方 `cooking/fast_burn_time_reduction_factor`：快速炉把燃烧时间除以它。
const FAST_BURN_TIME_REDUCTION_FACTOR: i32 = 2;

/// 官方 `ResolvableInt.get`：常量直接用，引用查注册表，查不到时用调用方给的默认值。
fn resolve_int(value: &ResolvableInt, fast_cooking: bool, default: i32) -> i32 {
    match value {
        ResolvableInt::Constant(value) => *value,
        ResolvableInt::Reference(key) => int_reference(key, fast_cooking).unwrap_or(default),
    }
}

/// 官方 `ResolvableFloat.get`，语义和 [`resolve_int`] 一样。
fn resolve_float(value: &ResolvableFloat, fast_cooking: bool, default: f32) -> f32 {
    match value {
        ResolvableFloat::Constant(value) => *value,
        ResolvableFloat::Reference(key) => float_reference(key, fast_cooking).unwrap_or(default),
    }
}

/// 官方 `ContextIntProviders.bootstrap` 注册的数值。
///
/// `cooking/time_*` 是篝火以外的燃料 tick 数，再按 `FAST_FURNACE` 谓词做整数除法；
/// 谓词为真就是高炉和烟熏炉。未知的键返回 `None`，调用方用默认值。
fn int_reference(key: &str, fast_cooking: bool) -> Option<i32> {
    let key = key.strip_prefix("minecraft:").unwrap_or(key);
    if let Some(time) = key.strip_prefix("cooking/time_") {
        let time = match time {
            "bamboo" => 50,
            "wool_slabs" => 50,
            "wool_carpets" => 67,
            "dry_plants" => 100,
            "wood_items_extra_small" => 100,
            "wool" => 100,
            "wood_slabs" => 150,
            "wood_items_large" => 200,
            "roots" => 300,
            "wood_blocks" => 300,
            "wood_items_small" => 300,
            "hanging_signs" => 800,
            "boats" => 1200,
            "coal" => 1600,
            "blaze_rod" => 2400,
            "dried_kelp_block" => 4001,
            "coal_block" => 16000,
            "lava_bucket" => 20000,
            _ => return None,
        };
        // 官方是 `Quotient(exactly(time), ConditionalValue(FAST_FURNACE, 2, 1))`，整数除法。
        return Some(
            time / if fast_cooking {
                FAST_BURN_TIME_REDUCTION_FACTOR
            } else {
                1
            },
        );
    }
    Some(match key {
        "cooking/normal_burn_time_reduction_factor" => 1,
        "cooking/fast_burn_time_reduction_factor" => 2,
        "brewing/uses_default" => 20,
        _ => return None,
    })
}

/// 官方 `ContextFloatProviders.bootstrap` 注册的数值。
fn float_reference(key: &str, fast_cooking: bool) -> Option<f32> {
    let key = key.strip_prefix("minecraft:").unwrap_or(key);
    Some(match key {
        "cooking/speed_default" => {
            if fast_cooking {
                2.0
            } else {
                1.0
            }
        }
        "cooking/normal_speed_multiplier" => 1.0,
        "cooking/fast_speed_multiplier" => 2.0,
        "brewing/speed_default" => 1.0,
        _ => return None,
    })
}

/// `cooking_fuel` 组件求值后的燃烧时间和速度乘数，官方 `getBurnDuration` 和
/// `getSpeedMultiplier` 一起取。
///
/// 没有组件时返回 `None`，调用方按 `(0, 1.0)` 处理（官方 `getFromItem` 的默认值）。
/// `fast_cooking` 是高炉和烟熏炉：燃料的 `cooking/time_*` 引用会除以 2。
pub fn cooking_fuel(stack: &ItemStack, fast_cooking: bool) -> Option<(i32, f32)> {
    match crate::matching::resolved(stack, kinds::COOKING_FUEL)? {
        ComponentValue::CookingFuel {
            burn_time,
            speed_multiplier,
        } => Some((
            resolve_int(&burn_time, fast_cooking, 0),
            resolve_float(&speed_multiplier, fast_cooking, 1.0),
        )),
        _ => None,
    }
}

/// `brewing_fuel` 组件求值后的使用次数和速度乘数。
///
/// 没有组件时返回 `None`，调用方按 `(0, 1.0)` 处理。
pub fn brewing_fuel(stack: &ItemStack) -> Option<(i32, f32)> {
    match crate::matching::resolved(stack, kinds::BREWING_FUEL)? {
        ComponentValue::BrewingFuel {
            uses,
            speed_multiplier,
        } => Some((
            resolve_int(&uses, false, 0),
            resolve_float(&speed_multiplier, false, 1.0),
        )),
        _ => None,
    }
}

/// 物品有没有 `cooking_fuel` 组件，官方 `stack.has(DataComponents.COOKING_FUEL)`。
pub fn is_cooking_fuel(stack: &ItemStack) -> bool {
    crate::matching::resolved(stack, kinds::COOKING_FUEL).is_some()
}

/// 物品有没有 `brewing_fuel` 组件。
pub fn is_brewing_fuel(stack: &ItemStack) -> bool {
    crate::matching::resolved(stack, kinds::BREWING_FUEL).is_some()
}

/// 官方 `Item.getCraftingRemainder()`：物品上的 `craftingRemainingItem`。
///
/// Vanilla `Item.getCraftingRemainder()`: the item's `craftingRemainingItem` field.
pub fn crafting_remainder(stack: &ItemStack) -> Option<ItemStack> {
    crate::assemble::remainder(stack)
}

/// 物品栈的 `max_stack_size`，官方 `ItemStack.getMaxStackSize`。
pub fn max_stack_size(stack: &ItemStack) -> i32 {
    mc_bamboo_protocol::components::validate::max_stack_size(
        stack.item,
        &crate::assemble::patch_of(stack),
    )
}

/// 两个栈是不是同一个物品带同一份组件，官方 `ItemStack.isSameItemSameComponents`。
///
/// 数量不参与比较；组件按映射比，录入顺序不同也算相等。
pub fn same_item_same_components(a: &ItemStack, b: &ItemStack) -> bool {
    if a.item != b.item {
        return false;
    }
    if a.is_empty() && b.is_empty() {
        return true;
    }
    match (
        ComponentPatch::from_opaque(a),
        ComponentPatch::from_opaque(b),
    ) {
        (Ok(a), Ok(b)) => {
            a.entries().len() == b.entries().len()
                && a.entries()
                    .iter()
                    .all(|(kind, entry)| b.entry(*kind) == Some(entry))
        }
        // 解不开的组件按字节比，至少不会把明显不同的栈当成一样。
        _ => a.added == b.added && a.removed == b.removed,
    }
}

/// 官方 `RecipePropertySet.BREWING_INPUTS`：酿造配方输入侧的全部物品。
pub fn brewing_inputs() -> &'static [i32] {
    static SET: OnceLock<Vec<i32>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut items = Vec::new();
        for recipe in RECIPES {
            if let RecipeKind::Brewing { input, .. } = &recipe.kind {
                items.extend_from_slice(input.item.items);
            }
        }
        items.sort_unstable();
        items.dedup();
        items
    })
}

/// 官方 `RecipePropertySet.BREWING_REAGENTS`：酿造配方材料侧的全部物品。
pub fn brewing_reagents() -> &'static [i32] {
    static SET: OnceLock<Vec<i32>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut items = Vec::new();
        for recipe in RECIPES {
            if let RecipeKind::Brewing { reagent, .. } = &recipe.kind {
                items.extend_from_slice(reagent.item.items);
            }
        }
        items.sort_unstable();
        items.dedup();
        items
    })
}

/// 物品能不能放进酿造台的材料格，官方 `BREWING_REAGENTS.test(stack)`。
pub fn is_brewing_reagent(item: i32) -> bool {
    brewing_reagents().binary_search(&item).is_ok()
}

/// 官方 `PotionIngredient.isPotionInput`：酿造配方输入侧的物品，或者
/// `minecraft:brewing_potion_inputs` 标签里的物品。
pub fn is_brewing_potion_input(item: i32) -> bool {
    brewing_inputs().binary_search(&item).is_ok()
        || item_tag_contains(item_tags::BREWING_POTION_INPUTS, item)
}

/// 官方 `ItemTag.test`。
pub fn item_tag_contains(tag: ItemTag, item: i32) -> bool {
    TAG_GROUPS[item_tags::GROUP].tags[tag.index]
        .ids
        .contains(&item)
}

/// 官方 `RecipePropertySet.SMITHING_TEMPLATE`。
pub fn is_smithing_template(item: i32) -> bool {
    static SET: OnceLock<Vec<i32>> = OnceLock::new();
    let set = SET.get_or_init(|| {
        let mut items = Vec::new();
        for recipe in RECIPES {
            match &recipe.kind {
                RecipeKind::SmithingTransform {
                    template: Some(template),
                    ..
                } => {
                    items.extend_from_slice(template.items);
                }
                RecipeKind::SmithingTransform { .. } => {}
                RecipeKind::SmithingTrim { template, .. } => {
                    items.extend_from_slice(template.items);
                }
                _ => {}
            }
        }
        items.sort_unstable();
        items.dedup();
        items
    });
    set.binary_search(&item).is_ok()
}

/// 官方 `RecipePropertySet.SMITHING_BASE`。
pub fn is_smithing_base(item: i32) -> bool {
    static SET: OnceLock<Vec<i32>> = OnceLock::new();
    let set = SET.get_or_init(|| {
        let mut items = Vec::new();
        for recipe in RECIPES {
            match &recipe.kind {
                RecipeKind::SmithingTransform { base, .. }
                | RecipeKind::SmithingTrim { base, .. } => {
                    items.extend_from_slice(base.items);
                }
                _ => {}
            }
        }
        items.sort_unstable();
        items.dedup();
        items
    });
    set.binary_search(&item).is_ok()
}

/// 官方 `RecipePropertySet.SMITHING_ADDITION`。
pub fn is_smithing_addition(item: i32) -> bool {
    static SET: OnceLock<Vec<i32>> = OnceLock::new();
    let set = SET.get_or_init(|| {
        let mut items = Vec::new();
        for recipe in RECIPES {
            match &recipe.kind {
                RecipeKind::SmithingTransform {
                    addition: Some(addition),
                    ..
                } => {
                    items.extend_from_slice(addition.items);
                }
                RecipeKind::SmithingTransform { .. } => {}
                RecipeKind::SmithingTrim { addition, .. } => {
                    items.extend_from_slice(addition.items);
                }
                _ => {}
            }
        }
        items.sort_unstable();
        items.dedup();
        items
    });
    set.binary_search(&item).is_ok()
}

/// 官方 `RecipePropertySet.FURNACE_INPUT`、`BLAST_FURNACE_INPUT`、
/// `SMOKER_INPUT`、`CAMPFIRE_INPUT` 之一：这个烹饪类型的全部原料物品。
pub fn is_cooking_input(kind: CookingKind, item: i32) -> bool {
    static SETS: OnceLock<[Vec<i32>; 4]> = OnceLock::new();
    let sets = SETS.get_or_init(|| {
        let mut sets: [Vec<i32>; 4] = std::array::from_fn(|_| Vec::new());
        for recipe in RECIPES {
            if let RecipeKind::Cooking {
                kind, ingredient, ..
            } = &recipe.kind
            {
                sets[cooking_index(*kind)].extend_from_slice(ingredient.items);
            }
        }
        for set in &mut sets {
            set.sort_unstable();
            set.dedup();
        }
        sets
    });
    sets[cooking_index(kind)].binary_search(&item).is_ok()
}

const fn cooking_index(kind: CookingKind) -> usize {
    match kind {
        CookingKind::Smelting => 0,
        CookingKind::Blasting => 1,
        CookingKind::Smoking => 2,
        CookingKind::Campfire => 3,
    }
}

/// 官方 `RecipePropertySet.STONECUTTER_INPUT`：切石机配方的全部原料物品。
pub fn accepts_stonecutter_input(item: i32) -> bool {
    static SET: OnceLock<Vec<i32>> = OnceLock::new();
    let set = SET.get_or_init(|| {
        let mut items = Vec::new();
        for recipe in RECIPES {
            if let RecipeKind::Stonecutting { ingredient, .. } = &recipe.kind {
                items.extend_from_slice(ingredient.items);
            }
        }
        items.sort_unstable();
        items.dedup();
        items
    });
    set.binary_search(&item).is_ok()
}

/// 一个输入物品在切石机里的全部配方，按注册表顺序。
pub fn stonecutter_recipes(item: i32) -> impl Iterator<Item = &'static Recipe> {
    crate::RecipeManager::global()
        .recipes_of(RecipeType::Stonecutting)
        .filter(move |recipe| match &recipe.kind {
            RecipeKind::Stonecutting { ingredient, .. } => ingredient.test(item),
            _ => false,
        })
}

/// 物品的 `equippable` 组件给的装备槽 ID，没有组件时是 `None`。
pub fn equipment_slot(stack: &ItemStack) -> Option<i32> {
    match crate::matching::resolved(stack, kinds::EQUIPPABLE)? {
        ComponentValue::Equippable(equippable) => Some(equippable.slot),
        _ => None,
    }
}

/// 官方 `LivingEntity.getEquipmentSlotForItem`：没有 `equippable` 组件就是主手。
pub fn equipment_slot_for_item(stack: &ItemStack) -> i32 {
    equipment_slot(stack).unwrap_or(0)
}

/// 官方 `LivingEntity.isEquippableInSlot`，玩家版本。
///
/// 玩家能在所有槽位放东西；`canBeEquippedBy` 的 `allowed_entities` 没有复刻，
/// 见 README 的已知限制。
pub fn is_equippable_in_slot(stack: &ItemStack, slot: i32) -> bool {
    match equipment_slot(stack) {
        Some(equippable_slot) => equippable_slot == slot,
        None => slot == 0,
    }
}
