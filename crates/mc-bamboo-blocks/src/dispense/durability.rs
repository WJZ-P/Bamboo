//! `ItemStack.hurtAndBreak` 在发射器里的那一截。
//!
//! 通用耐久还没合并。这里只覆盖 `player == null` 且没有附魔的判定：不可破坏或没有耐久的物品不动，
//! 其余加上 `amount`，破了就少一个。`onBreak` 在打火石和剪刀里是空的。
//! 有附魔时官方会走 `EnchantmentHelper.processDurabilityChange`，可能改掉数量并取随机数，这里不取。
//!
//! The dispenser slice of `ItemStack.hurtAndBreak`.
//!
//! Shared durability is not merged yet. This covers `player == null` with no enchantments:
//! an unbreakable item or one without durability is unchanged, otherwise `amount` is added,
//! and a broken stack shrinks by one. `onBreak` is empty for flint and shears.
//! With enchantments, official runs `EnchantmentHelper.processDurabilityChange`, which can
//! change the amount and draw random numbers. This function does not.

use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults},
    item::ItemStack,
};

use crate::{dispenser::without_one, item_exchange::set_component};

/// `hurtAndBreak(amount, level, null, item -> {})`。
///
/// `hurtAndBreak(amount, level, null, item -> {})`.
pub(super) fn hurt_and_break(mut stack: ItemStack, amount: i32) -> ItemStack {
    let Some((damage, max_damage)) = damage_of(&stack) else {
        return stack;
    };
    // 没有附魔时 `processDurabilityChange` 原样返回 `amount`，不取随机数。
    //
    // With no enchantments `processDurabilityChange` returns `amount` and draws nothing.
    if amount == 0 {
        return stack;
    }
    let new_damage = damage.saturating_add(amount).clamp(0, max_damage);
    set_component(
        &mut stack,
        kinds::DAMAGE,
        ComponentValue::Damage(new_damage),
    );
    if new_damage >= max_damage {
        stack = without_one(&stack);
    }
    stack
}

/// `isDamageableItem` 之后的当前耐久和上限。夹在 0 和上限之间，和 `getDamageValue` 一样。
fn damage_of(stack: &ItemStack) -> Option<(i32, i32)> {
    let patch = stack.patch();
    let int = |kind| match defaults::resolve(stack.item, &patch, kind) {
        Some(ComponentValue::MaxDamage(value) | ComponentValue::Damage(value)) => Some(*value),
        _ => None,
    };
    let max_damage = int(kinds::MAX_DAMAGE)?;
    let damage = int(kinds::DAMAGE)?;
    if defaults::resolve(stack.item, &patch, kinds::UNBREAKABLE).is_some() {
        return None;
    }
    Some((damage.clamp(0, max_damage), max_damage))
}
