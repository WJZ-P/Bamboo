//! `ItemStack.hurtAndBreak` 和 `EnchantmentHelper.processDurabilityChange`。
//!
//! 玩家的两只手由调用方放进线程局部变量：方块逻辑在官方取随机数的那个位置扣耐久，
//! 不把物品堆一路传进每个 `World`。没有手时 [`hurt_hand`] 什么也不做。
//! 发射器没有玩家，走 [`hurt_and_break`]：`player == null`，耐久附魔仍用 `level.random`。
//!
//! `ItemStack.hurtAndBreak` and `EnchantmentHelper.processDurabilityChange`.
//!
//! The caller installs the player's two hands in a thread local, so block logic spends durability
//! at the same point vanilla draws its random numbers, without threading an item stack through
//! every `World`. [`hurt_hand`] does nothing when no hands are installed.
//! A dispenser has no player and uses [`hurt_and_break`]: `player == null`, and durability
//! enchantments still draw from `level.random`.

use std::cell::RefCell;

use mc_bamboo_core::Hand;
use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_data::tags::block_tags;
use mc_bamboo_data::{
    ItemDamageEffect, ItemKind, LevelBasedValue, ToolMatch, ValueEffect, item_damage,
};
use mc_bamboo_protocol::components::validate::max_stack_size;
use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry, defaults};
use mc_bamboo_protocol::item::ItemStack;

use crate::behaviour::common::item_of;
use crate::context::UseContext;
use crate::random::LegacyRandom;
use crate::state::{BlockState, item_is_tagged};
use crate::world::World;

std::thread_local! {
    static HANDS: RefCell<Option<ActiveHands>> = const { RefCell::new(None) };
}

/// 一次动作里玩家的两只手。
///
/// The player's two hands for one action.
#[derive(Debug, Clone)]
pub struct ActiveHands {
    pub main: ItemStack,
    pub off: ItemStack,
    /// `Abilities.instabuild`。创造模式是真，冒险模式和生存模式是假。
    ///
    /// `Abilities.instabuild`. True in creative, false in adventure and survival.
    pub infinite_materials: bool,
    /// 这一次动作里损坏的手，按损坏顺序。
    ///
    /// Hands that broke during this action, in the order they broke.
    pub broken: Vec<Hand>,
}

impl ActiveHands {
    fn stack(&self, hand: Hand) -> &ItemStack {
        match hand {
            Hand::Main => &self.main,
            Hand::Off => &self.off,
        }
    }

    fn stack_mut(&mut self, hand: Hand) -> &mut ItemStack {
        match hand {
            Hand::Main => &mut self.main,
            Hand::Off => &mut self.off,
        }
    }
}

/// 跑 `body` 时线程上有这双手。`body` 恐慌也会把原来的手放回去。
///
/// `body` runs with these hands installed. A panic still restores the previous hands.
pub fn with_player_hands<R>(hands: ActiveHands, body: impl FnOnce() -> R) -> (R, ActiveHands) {
    struct Restore(Option<ActiveHands>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            HANDS.with(|slot| *slot.borrow_mut() = previous);
        }
    }

    let previous = HANDS.with(|slot| slot.borrow_mut().replace(hands));
    let restore = Restore(previous);
    let result = body();
    let hands = HANDS.with(|slot| {
        slot.borrow()
            .clone()
            .expect("player hands stay installed until the action returns")
    });
    drop(restore);
    (result, hands)
}

/// `Item.mineBlock`。剪刀走 `ShearsItem.mineBlock`，不看方块硬度，只跳过火。
///
/// `Item.mineBlock`. Shears use `ShearsItem.mineBlock`: hardness is ignored and fire is skipped.
pub(crate) fn mine_block(world: &World, state: BlockState) {
    let Some(stack) = HANDS.with(|slot| slot.borrow().as_ref().map(|hands| hands.main.clone()))
    else {
        return;
    };
    let item = stack.item;
    let Some(damage_per_block) = tool_damage_per_block(&stack) else {
        return;
    };
    if damage_per_block <= 0 {
        return;
    }
    let spends = if item_of(item).is_some_and(|item| item.kind == ItemKind::Shears) {
        !state.is_tagged(block_tags::FIRE)
    } else {
        state.block().destroy_time != 0.0
    };
    if spends {
        hurt_hand(world, Hand::Main, damage_per_block);
    }
}

/// `hurtAndBreak(amount, player, hand)`。没有手时什么也不做。
///
/// `hurtAndBreak(amount, player, hand)`. Does nothing when no hands are installed.
pub(crate) fn hurt_hand(world: &World, hand: Hand, amount: i32) {
    edit_hand(hand, |stack, hands| {
        let infinite = hands.infinite_materials;
        hurt_stack(
            stack,
            amount,
            world.random(),
            infinite,
            hand,
            &mut hands.broken,
        );
    });
}

/// `hurtAndBreak(amount, level, null, onBreak)`。`onBreak` 是空的。
///
/// `player == null` 时不看创造模式，耐久附魔仍走 `EnchantmentHelper.processDurabilityChange`，
/// 随机数是 `level.random`。
///
/// `hurtAndBreak(amount, level, null, onBreak)`. `onBreak` is empty.
///
/// `player == null` skips the creative-mode check. Durability enchantments still run through
/// `EnchantmentHelper.processDurabilityChange`, and the draws come from `level.random`.
pub(crate) fn hurt_and_break(
    mut stack: ItemStack,
    amount: i32,
    random: &LegacyRandom,
) -> ItemStack {
    let Ok(patch) = ComponentPatch::from_opaque(&stack) else {
        return stack;
    };
    let spent = durability_change(stack.item, &patch, amount, random, false);
    if spent == 0 {
        return stack;
    }
    let new_damage = damage_value(stack.item, &patch).wrapping_add(spent);
    if write_damage(&mut stack, new_damage) {
        stack.count -= 1;
    }
    stack
}

/// `ItemStack.consume`：创造模式不减。
///
/// `ItemStack.consume`: creative mode does not shrink the stack.
pub(crate) fn consume_hand(hand: Hand, amount: i32) {
    edit_hand(hand, |stack, hands| {
        if !hands.infinite_materials {
            stack.count -= amount;
        }
    });
}

/// `ItemStack.shrink`。`FireChargeItem.useOn` 用的是这个，创造模式的数量由外层再改回去。
///
/// `ItemStack.shrink`. `FireChargeItem.useOn` uses this; creative mode restores the count outside.
pub(crate) fn shrink_hand(hand: Hand, amount: i32) {
    edit_hand(hand, |stack, _hands| {
        stack.count -= amount;
    });
}

/// 这只手上的物品堆。没有手时是 `None`。
///
/// The stack in this hand. `None` when no hands are installed.
fn edit_hand(hand: Hand, body: impl FnOnce(&mut ItemStack, &mut ActiveHands)) {
    HANDS.with(|slot| {
        let mut hands = slot.borrow_mut();
        let Some(hands) = hands.as_mut() else {
            return;
        };
        let mut stack = hands.stack(hand).clone();
        body(&mut stack, hands);
        *hands.stack_mut(hand) = stack;
    });
}

/// `ItemStack.hurtAndBreak` 的那一支。
fn hurt_stack(
    stack: &mut ItemStack,
    amount: i32,
    random: &LegacyRandom,
    infinite_materials: bool,
    hand: Hand,
    broken: &mut Vec<Hand>,
) {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return;
    };
    let spent = durability_change(stack.item, &patch, amount, random, infinite_materials);
    if spent == 0 {
        return;
    }
    let new_damage = damage_value(stack.item, &patch).wrapping_add(spent);
    if write_damage(stack, new_damage) {
        stack.count -= 1;
        broken.push(hand);
    }
}

/// `processDurabilityChange`。创造模式在附魔取数之前就返回。
///
/// `processDurabilityChange`. Creative mode returns before the enchantment draws.
fn durability_change(
    item: i32,
    patch: &ComponentPatch,
    amount: i32,
    random: &LegacyRandom,
    infinite_materials: bool,
) -> i32 {
    if !is_damageable(item, patch) || infinite_materials {
        return 0;
    }
    if amount <= 0 {
        return amount;
    }
    let mut value = amount as f32;
    if let Some(ComponentValue::Enchantments(levels)) =
        defaults::resolve(item, patch, kinds::ENCHANTMENTS)
    {
        for &(enchantment, level) in levels {
            value = apply_item_damage(enchantment, level, item, random, value);
        }
    }
    value as i32
}

/// `Enchantment.modifyDurabilityChange`：按物品上的顺序套用这个附魔的效果。
///
/// `Enchantment.modifyDurabilityChange`: apply this enchantment's effects in item order.
fn apply_item_damage(
    enchantment: i32,
    level: i32,
    item: i32,
    random: &LegacyRandom,
    value: f32,
) -> f32 {
    let Some(effects) = item_damage::ITEM_DAMAGE
        .iter()
        .find(|entry| entry.enchantment == enchantment)
        .map(|entry| entry.effects)
    else {
        return value;
    };
    effects.iter().fold(value, |value, effect| {
        if effect_matches(effect, item) {
            apply_effect(effect.effect, level, random, value)
        } else {
            value
        }
    })
}

fn effect_matches(effect: &ItemDamageEffect, item: i32) -> bool {
    effect
        .requirement
        .is_none_or(|requirement| tool_matches(requirement, item))
}

fn tool_matches(matcher: ToolMatch, item: i32) -> bool {
    match matcher {
        ToolMatch::Any => true,
        ToolMatch::Tag(tag) => item_is_tagged(item, tag),
        ToolMatch::Items(ids) => ids.contains(&item),
        ToolMatch::Inverted(inner) => !tool_matches(*inner, item),
        ToolMatch::AllOf(terms) => terms.iter().all(|term| tool_matches(*term, item)),
        ToolMatch::AnyOf(terms) => terms.iter().any(|term| tool_matches(*term, item)),
    }
}

fn apply_effect(effect: ValueEffect, level: i32, random: &LegacyRandom, input: f32) -> f32 {
    match effect {
        ValueEffect::Add(value) => input + calculate(value, level),
        ValueEffect::Multiply(factor) => input * calculate(factor, level),
        ValueEffect::Set(value) => calculate(value, level),
        ValueEffect::RemoveBinomial { chance } => {
            remove_binomial(calculate(chance, level), random, input)
        }
        ValueEffect::Exponential { base, exponent } => {
            (f64::from(input)
                * f64::from(calculate(base, level)).powf(f64::from(calculate(exponent, level))))
                as f32
        }
        ValueEffect::AllOf(effects) => effects.iter().fold(input, |input, effect| {
            apply_effect(*effect, level, random, input)
        }),
    }
}

/// `LevelBasedValue.calculate`。
fn calculate(value: LevelBasedValue, level: i32) -> f32 {
    match value {
        LevelBasedValue::Constant(number) => number,
        LevelBasedValue::Linear {
            base,
            per_level_above_first,
        } => base + per_level_above_first * level.wrapping_sub(1) as f32,
        LevelBasedValue::Fraction {
            numerator,
            denominator,
        } => {
            let denominator = calculate(*denominator, level);
            if denominator == 0.0 {
                0.0
            } else {
                calculate(*numerator, level) / denominator
            }
        }
        LevelBasedValue::Clamped { value, min, max } => {
            mth_clamp(calculate(*value, level), min, max)
        }
        LevelBasedValue::LevelsSquared { added } => level.wrapping_mul(level) as f32 + added,
        LevelBasedValue::Exponent { base, power } => {
            f64::from(calculate(*base, level)).powf(f64::from(calculate(*power, level))) as f32
        }
        LevelBasedValue::Lookup { values, fallback } => {
            if level >= 1 && (level as usize) <= values.len() {
                values[level as usize - 1]
            } else {
                calculate(*fallback, level)
            }
        }
    }
}

/// `Mth.clamp`。`max < min` 时结果是 `max`，和 `Math.min(Math.max(...))` 一样。
///
/// `Mth.clamp`. When `max < min` the result is `max`, matching `Math.min(Math.max(...))`.
fn mth_clamp(value: f32, min: f32, max: f32) -> f32 {
    if value.is_nan() || min.is_nan() || max.is_nan() {
        f32::NAN
    } else {
        value.max(min).min(max)
    }
}

/// `RemoveBinomial.process`。`n` 不大时对每个点取一次 `nextFloat`，比较用的是浮点 `n`。
///
/// `RemoveBinomial.process`. Small `n` draws one `nextFloat` per point, compared against float `n`.
fn remove_binomial(chance: f32, random: &LegacyRandom, n: f32) -> f32 {
    let mut dropped = 0;
    if n <= 128.0 || n * chance < 20.0 || n * (1.0 - chance) < 20.0 {
        let mut point = 0i32;
        while (point as f32) < n {
            if random.next_float() < chance {
                dropped += 1;
            }
            point = point.wrapping_add(1);
        }
    } else {
        let mean = f64::from(n * chance).floor();
        let sigma = f64::from(n * chance * (1.0 - chance)).sqrt();
        dropped = java_round_i32(mean + random.next_gaussian() * sigma);
        let limit = n as i32;
        if limit >= 0 {
            dropped = dropped.max(0).min(limit);
        }
    }
    n - dropped as f32
}

/// `Math.round(double)` 再收成 `int`：`floor(x + 0.5)` 的低 32 位。
///
/// `Math.round(double)` narrowed to `int`: the low 32 bits of `floor(x + 0.5)`.
fn java_round_i32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let shifted = value + 0.5;
    if !shifted.is_finite() {
        return 0;
    }
    (shifted.floor() as i64) as i32
}

/// `applyDamage` 里写耐久的那一步。坏了返回真，数量由调用方减，`onBreak` 也由调用方决定。
///
/// The damage write inside `applyDamage`. Returns true when the stack broke. The caller shrinks
/// it and decides what `onBreak` does.
fn write_damage(stack: &mut ItemStack, new_damage: i32) -> bool {
    let Ok(mut patch) = ComponentPatch::from_opaque(stack) else {
        return false;
    };
    let max = max_damage(stack.item, &patch);
    patch.put(
        kinds::DAMAGE,
        PatchEntry::Set(ComponentValue::Damage(new_damage.max(0).min(max))),
    );
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
    is_broken(stack)
}

/// `isDamageableItem`。
fn is_damageable(item: i32, patch: &ComponentPatch) -> bool {
    component_int(item, patch, kinds::MAX_DAMAGE).is_some()
        && defaults::resolve(item, patch, kinds::UNBREAKABLE).is_none()
        && component_int(item, patch, kinds::DAMAGE).is_some()
}

fn is_broken(stack: &ItemStack) -> bool {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return false;
    };
    is_damageable(stack.item, &patch)
        && damage_value(stack.item, &patch) >= max_damage(stack.item, &patch)
}

/// `getDamageValue`。
fn damage_value(item: i32, patch: &ComponentPatch) -> i32 {
    let damage = component_int(item, patch, kinds::DAMAGE).unwrap_or(0);
    damage.max(0).min(max_damage(item, patch))
}

fn max_damage(item: i32, patch: &ComponentPatch) -> i32 {
    component_int(item, patch, kinds::MAX_DAMAGE).unwrap_or(0)
}

fn component_int(item: i32, patch: &ComponentPatch, kind: i32) -> Option<i32> {
    match defaults::resolve(item, patch, kind) {
        Some(ComponentValue::MaxDamage(value) | ComponentValue::Damage(value)) => Some(*value),
        _ => None,
    }
}

fn tool_damage_per_block(stack: &ItemStack) -> Option<i32> {
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    match defaults::resolve(stack.item, &patch, kinds::TOOL) {
        Some(ComponentValue::Tool {
            damage_per_block, ..
        }) => Some(*damage_per_block),
        _ => None,
    }
}

/// `ItemStack.isStackable`。
pub(crate) fn is_stackable(stack: &ItemStack) -> bool {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return false;
    };
    max_stack_size(stack.item, &patch) > 1
        && !(is_damageable(stack.item, &patch) && damage_value(stack.item, &patch) > 0)
}

/// 右键改方块之前，按这一项规则消耗物品。
///
/// Before a block transform is written, spend the item the way this rule says.
pub(crate) fn pay_for_transform(
    world: &World,
    ctx: &UseContext,
    consume_on_use: bool,
    item_damage_per_use: i32,
) {
    let Some(stack) = HANDS.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|hands| hands.stack(ctx.hand).clone())
    }) else {
        return;
    };
    if is_stackable(&stack) {
        let amount = if consume_on_use { 1 } else { 0 };
        consume_hand(ctx.hand, amount);
    } else {
        hurt_hand(world, ctx.hand, item_damage_per_use);
    }
}

/// 读出夹好的耐久。不能损坏时是 `None`。
///
/// The clamped damage, or `None` when the stack is not damageable.
pub fn damage_of(stack: &ItemStack) -> Option<i32> {
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    is_damageable(stack.item, &patch).then(|| damage_value(stack.item, &patch))
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Direction};
    use mc_bamboo_data::{blocks, items, tags::item_tags};
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

    use super::*;
    use crate::interact::InteractionResult;
    use crate::state::{AGE_25, LIT};
    use crate::testing::{TestLevel, actor_holding, break_block, top_hit, use_item};
    use crate::{BlockHit, destroy_block};

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn component_stack(item: i32, entries: &[(i32, ComponentValue)]) -> ItemStack {
        let mut patch = ComponentPatch::new();
        for (kind, value) in entries {
            patch.put(*kind, PatchEntry::Set(value.clone()));
        }
        let (added, removed) = patch.to_opaque();
        ItemStack {
            item,
            count: 1,
            added,
            removed,
        }
    }

    fn plain(item: i32) -> ItemStack {
        ItemStack::new(item, 1)
    }

    fn holding(main: ItemStack, infinite_materials: bool) -> ActiveHands {
        ActiveHands {
            main,
            off: ItemStack::empty(),
            infinite_materials,
            broken: Vec::new(),
        }
    }

    fn survival(item: i32) -> crate::context::Actor {
        crate::context::Actor {
            prevents_block_drops: false,
            main_hand: item,
            ..actor_holding(item)
        }
    }

    fn max_of(item: i32) -> i32 {
        damage_limit(item)
    }

    fn damage_limit(item: i32) -> i32 {
        match defaults::of_item(item).get(kinds::MAX_DAMAGE) {
            Some(ComponentValue::MaxDamage(value)) => *value,
            _ => panic!("{item} should have max_damage"),
        }
    }

    fn unbreaking() -> i32 {
        let &[entry] = item_damage::ITEM_DAMAGE else {
            panic!("vanilla 26.3 has one item_damage enchantment");
        };
        entry.enchantment
    }

    fn enchanted(item: i32, level: i32) -> ItemStack {
        component_stack(
            item,
            &[(
                kinds::ENCHANTMENTS,
                ComponentValue::Enchantments(vec![(unbreaking(), level)]),
            )],
        )
    }

    #[test]
    fn mining_spends_one_and_hardness_zero_spends_nothing() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::STONE));
        let seed = level.random.source().seed();
        let actor = survival(items::DIAMOND_PICKAXE);
        let (_outcome, hands) =
            with_player_hands(holding(plain(items::DIAMOND_PICKAXE), false), || {
                destroy_block(
                    &mut level,
                    &mut crate::ticks::BlockTicks::new(),
                    &actor,
                    POS,
                )
            });
        assert!(level.get(POS).is_air());
        assert_eq!(damage_of(&hands.main), Some(1));
        assert_eq!(level.random.source().seed(), seed);
        assert!(hands.broken.is_empty());

        let mut soft = TestLevel::new();
        let grass = BlockState(blocks::SHORT_GRASS);
        assert_eq!(grass.block().destroy_time, 0.0);
        soft.set(POS, grass);
        let seed = soft.random.source().seed();
        let (_outcome, hands) =
            with_player_hands(holding(plain(items::DIAMOND_PICKAXE), false), || {
                destroy_block(&mut soft, &mut crate::ticks::BlockTicks::new(), &actor, POS)
            });
        assert_eq!(damage_of(&hands.main), Some(0));
        assert_eq!(soft.random.source().seed(), seed);
    }

    #[test]
    fn shears_ignore_hardness_and_skip_fire() {
        let actor = survival(items::SHEARS);
        let mut vine = TestLevel::new();
        vine.set(POS, BlockState(blocks::SHORT_GRASS));
        let (_outcome, hands) = with_player_hands(holding(plain(items::SHEARS), false), || {
            destroy_block(&mut vine, &mut crate::ticks::BlockTicks::new(), &actor, POS)
        });
        assert_eq!(damage_of(&hands.main), Some(1));

        let mut fire = TestLevel::new();
        fire.set(POS, BlockState(blocks::FIRE));
        let (_outcome, hands) = with_player_hands(holding(plain(items::SHEARS), false), || {
            destroy_block(&mut fire, &mut crate::ticks::BlockTicks::new(), &actor, POS)
        });
        assert_eq!(damage_of(&hands.main), Some(0));
    }

    #[test]
    fn creative_and_unbreakable_do_not_draw() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::STONE));
        let seed = level.random.source().seed();
        let creative = actor_holding(items::DIAMOND_PICKAXE);
        let (_outcome, hands) =
            with_player_hands(holding(enchanted(items::DIAMOND_PICKAXE, 3), true), || {
                destroy_block(
                    &mut level,
                    &mut crate::ticks::BlockTicks::new(),
                    &creative,
                    POS,
                )
            });
        assert_eq!(damage_of(&hands.main), Some(0));
        assert_eq!(level.random.source().seed(), seed);

        let mut direct = TestLevel::new();
        let seed = direct.random.source().seed();
        let (_result, hands) =
            with_player_hands(holding(enchanted(items::DIAMOND_PICKAXE, 3), true), || {
                let mut ticks = crate::ticks::BlockTicks::new();
                let world = World::new(&mut direct, &mut ticks);
                hurt_hand(&world, Hand::Main, 1);
            });
        assert_eq!(damage_of(&hands.main), Some(0));
        assert_eq!(direct.random.source().seed(), seed);

        let mut survival_level = TestLevel::new();
        survival_level.set(POS, BlockState(blocks::STONE));
        let seed = survival_level.random.source().seed();
        let unbreakable = component_stack(
            items::DIAMOND_PICKAXE,
            &[(kinds::UNBREAKABLE, ComponentValue::Unbreakable)],
        );
        let actor = survival(items::DIAMOND_PICKAXE);
        let (_outcome, hands) = with_player_hands(holding(unbreakable, false), || {
            destroy_block(
                &mut survival_level,
                &mut crate::ticks::BlockTicks::new(),
                &actor,
                POS,
            )
        });
        assert_eq!(damage_of(&hands.main), None);
        assert_eq!(survival_level.random.source().seed(), seed);
    }

    #[test]
    fn the_last_point_breaks_the_tool_and_zero_draws_nothing() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::STONE));
        let almost = component_stack(
            items::DIAMOND_PICKAXE,
            &[(
                kinds::DAMAGE,
                ComponentValue::Damage(max_of(items::DIAMOND_PICKAXE) - 1),
            )],
        );
        let actor = survival(items::DIAMOND_PICKAXE);
        let (_outcome, hands) = with_player_hands(holding(almost, false), || {
            destroy_block(
                &mut level,
                &mut crate::ticks::BlockTicks::new(),
                &actor,
                POS,
            )
        });
        assert!(hands.main.is_empty());
        assert_eq!(hands.broken, [Hand::Main]);

        let mut untouched = TestLevel::new();
        let seed = untouched.random.source().seed();
        let (_result, hands) =
            with_player_hands(holding(enchanted(items::DIAMOND_PICKAXE, 3), false), || {
                let mut ticks = crate::ticks::BlockTicks::new();
                // 直接打 0 点：`amount > 0` 才进附魔。
                // A direct 0: enchantments run only when `amount > 0`.
                let world = World::new(&mut untouched, &mut ticks);
                hurt_hand(&world, Hand::Main, 0);
            });
        assert_eq!(damage_of(&hands.main), Some(0));
        assert_eq!(untouched.random.source().seed(), seed);
    }

    #[test]
    fn unbreaking_uses_one_float_per_point_and_armor_uses_the_other_fraction() {
        let tool_level = 1;
        let expected = crate::random::LegacyRandom::new(0);
        let tool_saved = expected.next_float() < 0.5;
        let armor_saved = {
            let random = crate::random::LegacyRandom::new(0);
            random.next_float() < 0.2
        };

        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::STONE));
        let actor = survival(items::DIAMOND_PICKAXE);
        let (_outcome, hands) = with_player_hands(
            holding(enchanted(items::DIAMOND_PICKAXE, tool_level), false),
            || {
                destroy_block(
                    &mut level,
                    &mut crate::ticks::BlockTicks::new(),
                    &actor,
                    POS,
                )
            },
        );
        let spent = i32::from(!tool_saved);
        assert_eq!(damage_of(&hands.main), Some(spent));
        assert_eq!(level.random.source().seed(), expected.seed());
        assert!(item_is_tagged(
            items::DIAMOND_CHESTPLATE,
            item_tags::ENCHANTABLE_ARMOR
        ));
        assert!(!item_is_tagged(
            items::DIAMOND_PICKAXE,
            item_tags::ENCHANTABLE_ARMOR
        ));

        let mut armor_level = TestLevel::new();
        let (_result, hands) = with_player_hands(
            holding(enchanted(items::DIAMOND_CHESTPLATE, 1), false),
            || {
                hurt_hand(
                    &World::new(&mut armor_level, &mut crate::ticks::BlockTicks::new()),
                    Hand::Main,
                    1,
                );
            },
        );
        assert_eq!(damage_of(&hands.main), Some(i32::from(!armor_saved)));
        let armor_random = crate::random::LegacyRandom::new(0);
        armor_random.next_float();
        assert_eq!(armor_level.random.source().seed(), armor_random.seed());
    }

    fn use_with(level: &mut TestLevel, item: i32, stack: ItemStack, hit: BlockHit) -> ActiveHands {
        let actor = survival(item);
        let (_outcome, hands) =
            with_player_hands(holding(stack, false), || use_item(level, &actor, hit));
        hands
    }

    fn seed_without_holding(setup: impl Fn(&mut TestLevel)) -> i64 {
        let mut level = TestLevel::new();
        setup(&mut level);
        level.random.source().seed()
    }

    #[test]
    fn tool_use_spends_one_without_an_extra_draw() {
        let log = BlockState(blocks::OAK_LOG);
        let mut level = TestLevel::new();
        level.set(POS, log);
        let hands = use_with(
            &mut level,
            items::WOODEN_AXE,
            plain(items::WOODEN_AXE),
            top_hit(POS),
        );
        assert_eq!(damage_of(&hands.main), Some(1));
        assert_eq!(
            level.random.source().seed(),
            seed_without_holding(|level| {
                level.set(POS, log);
                use_item(level, &actor_holding(items::WOODEN_AXE), top_hit(POS));
            })
        );

        let mut dirt = TestLevel::new();
        dirt.set(POS, BlockState(blocks::DIRT));
        let hands = use_with(
            &mut dirt,
            items::WOODEN_HOE,
            plain(items::WOODEN_HOE),
            top_hit(POS),
        );
        assert_eq!(damage_of(&hands.main), Some(1));
        assert!(dirt.get(POS).is(blocks::FARMLAND));

        let mut path = TestLevel::new();
        path.set(POS, BlockState(blocks::DIRT));
        path.set(POS.above(), BlockState::AIR);
        let hands = use_with(
            &mut path,
            items::WOODEN_SHOVEL,
            plain(items::WOODEN_SHOVEL),
            top_hit(POS),
        );
        assert_eq!(damage_of(&hands.main), Some(1));
        assert!(path.get(POS).is(blocks::DIRT_PATH));
    }

    #[test]
    fn campfire_shovel_flint_shears_pumpkin_beehive_and_tnt_spend() {
        let mut campfire = TestLevel::new();
        campfire.set(POS, BlockState(blocks::CAMPFIRE).with(LIT, true));
        let hands = use_with(
            &mut campfire,
            items::WOODEN_SHOVEL,
            plain(items::WOODEN_SHOVEL),
            top_hit(POS),
        );
        assert!(!campfire.get(POS).get(LIT));
        assert_eq!(damage_of(&hands.main), Some(1));

        let mut flint = TestLevel::new();
        flint.set(POS, BlockState(blocks::CAMPFIRE).with(LIT, false));
        let bare = seed_without_holding(|level| {
            level.set(POS, BlockState(blocks::CAMPFIRE).with(LIT, false));
            use_item(level, &actor_holding(items::FLINT_AND_STEEL), top_hit(POS));
        });
        let hands = use_with(
            &mut flint,
            items::FLINT_AND_STEEL,
            plain(items::FLINT_AND_STEEL),
            top_hit(POS),
        );
        assert!(flint.get(POS).get(LIT));
        assert_eq!(damage_of(&hands.main), Some(1));
        assert_eq!(flint.random.source().seed(), bare);

        let mut vine = TestLevel::new();
        vine.set(POS, BlockState(blocks::TWISTING_VINES).with(AGE_25, 3));
        let hands = use_with(&mut vine, items::SHEARS, plain(items::SHEARS), top_hit(POS));
        assert_eq!(vine.get(POS).get(AGE_25), 25);
        assert_eq!(damage_of(&hands.main), Some(1));

        let mut pumpkin = TestLevel::new();
        pumpkin.set(POS, BlockState(blocks::PUMPKIN));
        let hands = use_with(
            &mut pumpkin,
            items::SHEARS,
            plain(items::SHEARS),
            top_hit(POS),
        );
        assert!(pumpkin.get(POS).is(blocks::CARVED_PUMPKIN));
        assert_eq!(damage_of(&hands.main), Some(1));

        let mut hive = TestLevel::new();
        hive.set(
            POS,
            BlockState(blocks::BEEHIVE).with(crate::state::LEVEL_HONEY, 5),
        );
        let hands = use_with(&mut hive, items::SHEARS, plain(items::SHEARS), top_hit(POS));
        assert_eq!(hive.get(POS).get(crate::state::LEVEL_HONEY), 0);
        assert_eq!(damage_of(&hands.main), Some(1));

        let mut tnt = TestLevel::new();
        tnt.set(POS, BlockState(blocks::TNT));
        let bare = seed_without_holding(|level| {
            level.set(POS, BlockState(blocks::TNT));
            use_item(level, &actor_holding(items::FLINT_AND_STEEL), top_hit(POS));
        });
        let hands = use_with(
            &mut tnt,
            items::FLINT_AND_STEEL,
            plain(items::FLINT_AND_STEEL),
            top_hit(POS),
        );
        assert!(tnt.get(POS).is_air());
        assert_eq!(damage_of(&hands.main), Some(1));
        assert_eq!(tnt.random.source().seed(), bare);

        let mut charge = TestLevel::new();
        charge.set(POS, BlockState(blocks::TNT));
        let stack = ItemStack::new(items::FIRE_CHARGE, 4);
        let hands = use_with(&mut charge, items::FIRE_CHARGE, stack, top_hit(POS));
        assert_eq!(hands.main.count, 3);
    }

    #[test]
    fn an_unaffected_copper_statue_spends_the_axe() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::COPPER_GOLEM_STATUE));
        let hands = use_with(
            &mut level,
            items::WOODEN_AXE,
            plain(items::WOODEN_AXE),
            top_hit(POS),
        );
        assert!(level.get(POS).is_air());
        assert_eq!(damage_of(&hands.main), Some(1));
    }

    #[test]
    fn a_failed_ignite_does_not_spend_and_creative_fire_charge_still_shrinks_here() {
        let mut level = TestLevel::new();
        let hit = BlockHit {
            face: Direction::North,
            ..top_hit(POS)
        };
        level.set(POS, BlockState(blocks::STONE));
        let hands = use_with(
            &mut level,
            items::FLINT_AND_STEEL,
            plain(items::FLINT_AND_STEEL),
            hit,
        );
        assert_eq!(damage_of(&hands.main), Some(0));

        let mut creative = TestLevel::new();
        creative.set(POS, BlockState(blocks::STONE));
        let actor = actor_holding(items::FIRE_CHARGE);
        let stack = ItemStack::new(items::FIRE_CHARGE, 4);
        let (outcome, hands) = with_player_hands(holding(stack, true), || {
            use_item(&mut creative, &actor, top_hit(POS))
        });
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(hands.main.count, 3);
    }

    #[test]
    fn breaking_without_a_player_draws_nothing_new() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::STONE));
        let seed = level.random.source().seed();
        break_block(&mut level, POS);
        assert_eq!(level.random.source().seed(), seed);
    }
}
