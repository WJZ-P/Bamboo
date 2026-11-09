//! 附魔台的三个选项和附魔结果，对应 `EnchantmentMenu` 与 `EnchantmentHelper`。
//!
//! 随机源是 `LegacyRandomSource`，每次用 `enchantmentSeed + slot` 或 `enchantmentSeed`
//! `setSeed` 之后重新取数，和官方一致。

use mc_bamboo_blocks::LegacyRandom;
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{items, registries::data_component_type as kinds, tags};
use mc_bamboo_protocol::{components::ComponentValue, item::ItemStack};

use super::{
    enchantments,
    item_patch::{assign, patch_of, resolved, write_patch},
};

/// `EnchantmentMenu` 的三个选项。
pub(super) const OPTIONS: usize = 3;
/// `DataSlot` 的顺序：costs[0..3], seed, clue[0..3], level[0..3]。
pub(super) const DATA_LEN: usize = 10;
pub(super) const SEED_SLOT: usize = 3;
pub(super) const CLUE_SLOT: usize = 4;
pub(super) const LEVEL_SLOT: usize = 7;

/// 一次加权抽中的附魔。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Instance {
    pub id: i32,
    pub level: i32,
}

/// `ItemStack.isEnchantable`：有 `enchantable` 组件，而且没有附魔。
pub(super) fn is_enchantable(stack: &ItemStack) -> bool {
    if resolved(stack, kinds::ENCHANTABLE).is_none() {
        return false;
    }
    match resolved(stack, kinds::ENCHANTMENTS) {
        Some(ComponentValue::Enchantments(levels)) => levels.is_empty(),
        _ => true,
    }
}

/// 官方 `getEnchantmentCost`。
pub(super) fn cost(random: &LegacyRandom, slot: i32, bookshelves: i32, stack: &ItemStack) -> i32 {
    let Some(ComponentValue::Enchantable(value)) = resolved(stack, kinds::ENCHANTABLE) else {
        return 0;
    };
    let _ = value; // 这个值由 `select` 使用，成本公式不读它。
    let bookshelves = bookshelves.min(15);
    let n = 1 + random.next_int(8) + bookshelves / 2 + random.next_int(bookshelves + 1);
    match slot {
        0 => (n / 3).max(1),
        1 => n * 2 / 3 + 1,
        _ => n.max(bookshelves * 2),
    }
}

/// `EnchantingTableBlock.BOOKSHELF_OFFSETS`：x/z -2..2、y 0..1，且 |x|==2 或 |z|==2。
fn bookshelf_offsets() -> Vec<BlockPos> {
    let mut result = Vec::new();
    for x in -2i32..=2 {
        for y in 0..=1 {
            for z in -2i32..=2 {
                if x.abs() == 2 || z.abs() == 2 {
                    result.push(BlockPos::new(x, y, z));
                }
            }
        }
    }
    result
}

/// `EnchantingTableBlock.isValidBookShelf` 的两次方块查找。
fn valid_bookshelf(
    block_at: impl Fn(BlockPos) -> Option<u32>,
    pos: BlockPos,
    offset: BlockPos,
) -> bool {
    let Some(outer) = block_at(pos.offset(offset.x, offset.y, offset.z)) else {
        return false;
    };
    if !mc_bamboo_blocks::BlockState(outer).is_tagged(tags::block_tags::ENCHANTMENT_POWER_PROVIDER)
    {
        return false;
    }
    let middle = pos.offset(offset.x / 2, offset.y, offset.z / 2);
    block_at(middle).is_some_and(|state| {
        mc_bamboo_blocks::BlockState(state)
            .is_tagged(tags::block_tags::ENCHANTMENT_POWER_TRANSMITTER)
    })
}

/// 数有效书架；`block_at` 读世界里的方块状态。
pub(super) fn bookshelf_count(block_at: impl Fn(BlockPos) -> Option<u32>, pos: BlockPos) -> i32 {
    bookshelf_offsets()
        .into_iter()
        .filter(|&offset| valid_bookshelf(&block_at, pos, offset))
        .count() as i32
}

/// `EnchantmentHelper.getAvailableEnchantmentResults`。
pub(super) fn available(level: i32, stack: &ItemStack) -> Vec<Instance> {
    let item = stack.item;
    let book = item == items::BOOK;
    let mut result = Vec::new();
    for &id in enchantments::in_table() {
        if !enchantments::is_primary(id, item) && !book {
            continue;
        }
        let definition = enchantments::get(id);
        let mut at = definition.max_level;
        while at >= 1 {
            if level >= definition.min_at(at) && level <= definition.max_at(at) {
                result.push(Instance { id, level: at });
                break;
            }
            at -= 1;
        }
    }
    result
}

/// `WeightedRandom.getRandomItem(random, list, EnchantmentInstance::weight)`。
fn weighted(random: &LegacyRandom, list: &[Instance]) -> Option<Instance> {
    let total: i32 = list
        .iter()
        .map(|entry| enchantments::get(entry.id).weight)
        .sum();
    if total <= 0 {
        return None;
    }
    let mut roll = random.next_int(total);
    for &entry in list {
        roll -= enchantments::get(entry.id).weight;
        if roll < 0 {
            return Some(entry);
        }
    }
    None
}

/// `EnchantmentHelper.selectEnchantment` 加上 `EnchantmentMenu.getEnchantmentList` 的书本裁剪。
pub(super) fn select(
    random: &LegacyRandom,
    stack: &ItemStack,
    _slot: i32,
    level: i32,
) -> Vec<Instance> {
    let Some(ComponentValue::Enchantable(value)) = resolved(stack, kinds::ENCHANTABLE) else {
        return Vec::new();
    };
    let mut level = level + 1 + random.next_int(value / 4 + 1) + random.next_int(value / 4 + 1);
    let delta = (random.next_float() + random.next_float() - 1.0) * 0.15;
    level = java_round(level as f32 * (1.0 + delta)).clamp(1, i32::MAX);
    let mut pool = available(level, stack);
    let mut selected = Vec::new();
    // 官方先在循环外抽一次，之后每轮 `nextInt(50)`、过滤、再抽一次。
    if let Some(first) = weighted(random, &pool) {
        selected.push(first);
        while random.next_int(50) <= level {
            let Some(last) = selected.last().copied() else {
                break;
            };
            pool.retain(|entry| enchantments::compatible(entry.id, last.id));
            if pool.is_empty() {
                break;
            }
            let Some(next) = weighted(random, &pool) else {
                break;
            };
            selected.push(next);
            level /= 2;
        }
    }
    if stack.item == items::BOOK && selected.len() > 1 {
        selected.remove(random.next_int(selected.len() as i32) as usize);
    }
    selected
}

/// Java `Math.round(float)`：floor(x + 0.5)，负数也照这个方向。
fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

/// `EnchantmentMenu.slotsChanged` 的 `access.execute` 部分，只算数据槽。
pub(super) fn refresh(
    random: &LegacyRandom,
    data: &mut [i32],
    stack: &ItemStack,
    bookshelves: i32,
) {
    if stack.is_empty() || !is_enchantable(stack) {
        for index in 0..OPTIONS {
            data[index] = 0;
            data[CLUE_SLOT + index] = -1;
            data[LEVEL_SLOT + index] = -1;
        }
        return;
    }
    random.set_seed(i64::from(data[SEED_SLOT]));
    for index in 0..OPTIONS {
        data[index] = cost(random, index as i32, bookshelves, stack);
        data[CLUE_SLOT + index] = -1;
        data[LEVEL_SLOT + index] = -1;
        if data[index] < index as i32 + 1 {
            data[index] = 0;
        }
    }
    for index in 0..OPTIONS {
        if data[index] <= 0 {
            continue;
        }
        // `EnchantmentMenu.getEnchantmentList` 每槽都重新 `setSeed(seed + slot)`。
        random.set_seed(i64::from(data[SEED_SLOT]) + index as i64);
        let list = select(random, stack, index as i32, data[index]);
        let Some(clue) = list.get(random.next_int(list.len() as i32) as usize) else {
            continue;
        };
        data[CLUE_SLOT + index] = clue.id;
        data[LEVEL_SLOT + index] = clue.level;
    }
}

/// `EnchantmentMenu.clickMenuButton` 的前半段校验。
pub(super) fn can_click(
    button: i32,
    data: &[i32],
    item: &ItemStack,
    lapis: &ItemStack,
    creative: bool,
    experience_level: i32,
) -> bool {
    if button < 0 || button as usize >= OPTIONS {
        return false;
    }
    let need = button + 1;
    if (lapis.is_empty() || lapis.count < need) && !creative {
        return false;
    }
    if data[button as usize] <= 0 || item.is_empty() {
        return false;
    }
    (experience_level >= need && experience_level >= data[button as usize]) || creative
}

/// 书变成附魔书：`transmuteCopy` 只复制补丁，别的组件走新物品的默认表。
pub(super) fn transmute_book(stack: &ItemStack) -> ItemStack {
    let mut result = ItemStack::new(items::ENCHANTED_BOOK, stack.count);
    let patch = patch_of(stack);
    write_patch(&mut result, &patch);
    result
}

/// `ItemStack.enchant`：`updateEnchantments` 读的是含默认组件的 `get`，
/// 附魔书走 `stored_enchantments`，别的物品走 `enchantments`。
pub(super) fn enchant(stack: &mut ItemStack, instances: &[Instance]) {
    let kind = if stack.item == items::ENCHANTED_BOOK {
        kinds::STORED_ENCHANTMENTS
    } else {
        kinds::ENCHANTMENTS
    };
    for entry in instances {
        let Some(ComponentValue::Enchantments(levels) | ComponentValue::StoredEnchantments(levels)) =
            resolved(stack, kind)
        else {
            continue;
        };
        let mut levels = levels;
        if let Some((_, stored)) = levels.iter_mut().find(|(id, _)| *id == entry.id) {
            *stored = (*stored).max(entry.level).min(255);
        } else {
            levels.push((entry.id, entry.level.min(255)));
        }
        let value = if kind == kinds::STORED_ENCHANTMENTS {
            ComponentValue::StoredEnchantments(levels)
        } else {
            ComponentValue::Enchantments(levels)
        };
        assign(stack, kind, Some(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_slot_layout_matches_the_constructor() {
        assert_eq!(DATA_LEN, 10);
        assert_eq!(SEED_SLOT, 3);
        assert_eq!(CLUE_SLOT, 4);
        assert_eq!(LEVEL_SLOT, 7);
        assert_eq!(bookshelf_offsets().len(), 32);
    }

    #[test]
    fn the_cost_formula_matches_three_slots() {
        let stack = ItemStack::new(items::DIAMOND_SWORD, 1);
        assert_eq!(cost(&LegacyRandom::new(7), 0, 15, &stack), 7);
        assert_eq!(cost(&LegacyRandom::new(7), 1, 15, &stack), 16);
        assert_eq!(cost(&LegacyRandom::new(7), 2, 15, &stack), 30);
    }

    #[test]
    fn a_book_with_no_enchantments_can_be_enchanted() {
        assert!(is_enchantable(&ItemStack::new(items::BOOK, 1)));
        assert!(!is_enchantable(&ItemStack::new(items::STONE, 1)));
    }

    #[test]
    fn clicking_needs_lapis_levels_and_a_positive_cost() {
        let data = [1, 2, 3, 0, -1, -1, -1, -1, -1, -1];
        let item = ItemStack::new(items::BOOK, 1);
        let lapis = ItemStack::new(items::LAPIS_LAZULI, 3);
        assert!(can_click(0, &data, &item, &lapis, false, 30));
        assert!(!can_click(0, &data, &item, &lapis, false, 0));
        assert!(can_click(2, &data, &item, &lapis, true, 0));
        assert!(!can_click(3, &data, &item, &lapis, true, 30));
    }
}
