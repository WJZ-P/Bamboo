//! 吃东西 / 喝东西：`Consumable.startConsuming` → `LivingEntity.updateUsingItem`
//! → `ItemStack.finishUsingItem` → `Consumable.onConsume`。
//!
//! 组件从物品的 [`defaults::resolve`] 读（补丁优先），不手写物品常量表。
//! 状态只存相对倒计时 `use_item_remaining`，拆岛 / 并岛不需要时钟平移。
//!
//! Eating and drinking, following vanilla's `Consumable` component path.
//! Only a relative countdown is stored.

use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, ComponentValue, defaults,
        value::{ConsumeEffect, EffectDetails, MobEffectInstance},
    },
    item::ItemStack,
};

use crate::{inbound::PlayerId, player::Player, region::Region};

/// 玩家正在使用一件可消耗物品。`remaining` 是剩余刻数（相对倒计时）。
///
/// One item being consumed; `remaining` is a relative countdown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UsingItem {
    pub hand: mc_bamboo_core::Hand,
    pub item: i32,
    pub remaining: i32,
    pub total: i32,
}

/// 一次使用能不能开始：`Consumable.canConsume` 的满饥饿判定，
/// 以及 `canAlwaysEat` / 创造模式。
///
/// `Consumable.canConsume`: full hunger, `canAlwaysEat`, creative mode.
pub(crate) fn can_start(player: &Player, stack: &ItemStack) -> Option<(i32, i32)> {
    if stack.is_empty() {
        return None;
    }
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    let kind = mc_bamboo_data::registries::data_component_type::CONSUMABLE;
    let ComponentValue::Consumable {
        consume_seconds, ..
    } = defaults::resolve(stack.item, &patch, kind)?
    else {
        return None;
    };
    let total = (consume_seconds * 20.0) as i32;
    if let Some(ComponentValue::Food { can_always_eat, .. }) = defaults::resolve(
        stack.item,
        &patch,
        mc_bamboo_data::registries::data_component_type::FOOD,
    ) && !*can_always_eat
        && player.health.food.food_level >= 20
        && player.game_mode != mc_bamboo_protocol::packet::play::GameMode::Creative
    {
        return None;
    }
    Some((stack.item, total))
}

/// `Consumable.startConsuming`：时长 > 0 时进「正在使用」状态；
/// 否则当场结算（0 刻的可消耗物品）。返回是否开始 / 已结算。
///
/// `Consumable.startConsuming`; zero-tick items settle at once.
pub(crate) fn start(region: &mut Region, id: PlayerId, hand: mc_bamboo_core::Hand) -> bool {
    let Some(player) = region.players.get_mut(&id) else {
        return false;
    };
    let stack = player.inventory.held(hand).clone();
    let Some((item, total)) = can_start(player, &stack) else {
        return false;
    };
    if total <= 0 {
        complete(region, id, hand, item);
        return true;
    }
    if player.use_item.is_some() || player.weapon_use.is_some() {
        return true;
    }
    player.use_item = Some(UsingItem {
        hand,
        item,
        remaining: total,
        total,
    });
    // `LivingEntity` 的使用标志在实体元数据里，观察者要看得见。
    player.sync.metadata_dirty = true;
    true
}

/// `LivingEntity.updateUsingItem`：每刻 --，到 0 时 `completeUsingItem()`。
///
/// `LivingEntity.updateUsingItem`: tick down and complete at zero.
pub(crate) fn tick(region: &mut Region) {
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in ids {
        let Some(player) = region.players.get_mut(&id) else {
            continue;
        };
        let Some(using) = player.use_item.as_mut() else {
            continue;
        };
        if using.remaining > 0 {
            using.remaining -= 1;
        }
        if using.remaining > 0 {
            continue;
        }
        let (hand, item) = (using.hand, using.item);
        player.use_item = None;
        player.sync.metadata_dirty = true;
        complete(region, id, hand, item);
    }
}

/// `Player.releaseUsingItem` / `Player.stopUsingItem`：中断不结算。
pub(crate) fn release(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id)
        && player.use_item.take().is_some()
    {
        player.sync.metadata_dirty = true;
    }
}

/// `ItemStack.finishUsingItem` → `Consumable.onConsume`：
/// 先 `FoodData.eat`，再按组件顺序应用 `on_consume_effects`，
/// 然后 `use_remainder` 替换手里的物品、减 1 个。
///
/// `ItemStack.finishUsingItem` → `Consumable.onConsume`.
pub(crate) fn complete(region: &mut Region, id: PlayerId, hand: mc_bamboo_core::Hand, item: i32) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let stack = player.inventory.held(hand).clone();
    if stack.is_empty() || stack.item != item {
        return;
    }
    let Ok(patch) = ComponentPatch::from_opaque(&stack) else {
        return;
    };
    // `FoodData.eat(nutrition, saturation)`。
    if let Some(ComponentValue::Food {
        nutrition,
        saturation,
        ..
    }) = defaults::resolve(item, &patch, kinds::FOOD)
    {
        crate::player_effects::eat_food(region, id, *nutrition, *saturation);
    }
    // 物品的 `potion_contents` / `suspicious_stew_effects` 自定义效果。
    apply_item_effects(region, id, item, &patch);
    // `Consumable.onConsume` 的 `on_consume_effects`。
    if let Some(ComponentValue::Consumable {
        on_consume_effects, ..
    }) = defaults::resolve(item, &patch, kinds::CONSUMABLE)
    {
        for effect in on_consume_effects.clone() {
            apply_consume_effect(region, id, &effect);
        }
    }
    // `ItemStack.consume(1, player)` + `use_remainder`。
    let remainder = match defaults::resolve(item, &patch, kinds::USE_REMAINDER) {
        Some(ComponentValue::UseRemainder(template)) => Some(template.clone()),
        _ => None,
    };
    if let Some(player) = region.players.get_mut(&id) {
        let slot = player.inventory.hand_slot(hand);
        let held = &mut player.inventory.slots[slot];
        held.count -= 1;
        if held.count <= 0 {
            *held = remainder.map_or_else(ItemStack::empty, |template| {
                let (added, removed) = template.components.to_opaque();
                ItemStack {
                    item: template.item,
                    count: template.count.max(1),
                    added,
                    removed,
                }
            });
        }
    }
    crate::inventory::sync_after_consume(region, id);
}

fn apply_consume_effect(region: &mut Region, id: PlayerId, effect: &ConsumeEffect) {
    match effect {
        ConsumeEffect::ApplyEffects {
            effects,
            probability,
        } => {
            if *probability < 1.0 {
                // 概率由玩家随机源决定；1.0 时不取数（官方都不取）。
                let roll = region
                    .players
                    .get_mut(&id)
                    .map(|player| player.random.next_float())
                    .unwrap_or(0.0);
                if roll >= *probability {
                    return;
                }
            }
            for instance in effects {
                let mut effect = mc_bamboo_blocks::entity::mob::effects::EffectInstance::with_flags(
                    instance.effect,
                    instance.details.duration,
                    instance.details.amplifier,
                    instance.details.ambient,
                    instance.details.show_particles,
                );
                // `ApplyEffects` 组件的 `show_icon` 是独立字段，不走官方五参数构造的缺省。
                effect.show_icon = instance.details.show_icon;
                crate::player_effects::give(region, id, effect);
            }
        }
        ConsumeEffect::ClearAllEffects => {
            crate::player_effects::clear_harmful(region, id);
        }
        ConsumeEffect::RemoveEffects { .. } | ConsumeEffect::PlaySound { .. } => {
            // 音效由客户端在吃 / 喝动画里播；移除指定效果要查 HolderSet，
            // 26.3 的可消耗物品里没有用到它的，留到下一轮。
        }
        ConsumeEffect::TeleportRandomly { .. } => {
            // 紫颂果的随机传送不做（见差异文档）。
        }
    }
}

/// `potion_contents` / `suspicious_stew_effects` 的自定义效果列表。
fn apply_item_effects(region: &mut Region, id: PlayerId, item: i32, patch: &ComponentPatch) {
    if let Some(ComponentValue::PotionContents { custom_effects, .. }) =
        defaults::resolve(item, patch, kinds::POTION_CONTENTS)
    {
        for instance in custom_effects {
            apply_custom_effect(region, id, instance);
        }
    }
    if let Some(ComponentValue::SuspiciousStewEffects(effects)) =
        defaults::resolve(item, patch, kinds::SUSPICIOUS_STEW_EFFECTS)
    {
        for effect in effects {
            apply_custom_effect(
                region,
                id,
                &MobEffectInstance {
                    effect: effect.effect,
                    details: EffectDetails {
                        amplifier: 0,
                        duration: effect.duration,
                        ambient: false,
                        show_particles: true,
                        show_icon: true,
                        hidden_effect: None,
                    },
                },
            );
        }
    }
}

fn apply_custom_effect(region: &mut Region, id: PlayerId, instance: &MobEffectInstance) {
    use mc_bamboo_data::registries::mob_effect;
    let d = &instance.details;
    // 瞬间效果走 `applyInstantaneousEffect`，不进 `activeEffects`。
    if matches!(
        instance.effect,
        mob_effect::INSTANT_HEALTH | mob_effect::INSTANT_DAMAGE | mob_effect::SATURATION
    ) {
        crate::player_effects::apply_instantaneous(region, id, instance.effect, d.amplifier);
    } else {
        crate::player_effects::give(
            region,
            id,
            mc_bamboo_blocks::entity::mob::effects::EffectInstance {
                effect: instance.effect,
                duration: d.duration,
                amplifier: d.amplifier,
                ambient: d.ambient,
                visible: d.show_particles,
                show_icon: d.show_icon,
                hidden: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_core::{Hand, Vec3};
    use mc_bamboo_data::{
        items,
        registries::{data_component_type as kinds, mob_effect},
    };
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    fn region_with_item(item: i32, count: i32) -> (Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::default(),
        );
        let slot = player.inventory.hand_slot(Hand::Main);
        player.inventory.slots[slot] = ItemStack::new(item, count);
        region.insert_player(player);
        (region, id)
    }

    #[test]
    fn the_official_default_components_drive_consumption() {
        let (region, id) = region_with_item(items::BREAD, 1);
        let bread = region.players[&id].inventory.held(Hand::Main).clone();
        let patch = ComponentPatch::from_opaque(&bread).unwrap();
        // 面包有 `food` 和 `consumable`，时长 32 刻。
        assert!(defaults::resolve(bread.item, &patch, kinds::FOOD).is_some());
        // 满饥饿时 `canAlwaysEat=false` 的面包不能开始；饥饿 19 时可以。
        assert_eq!(can_start(&region.players[&id], &bread), None);
        let (mut hungry, hungry_id) = region_with_item(items::BREAD, 1);
        hungry
            .players
            .get_mut(&hungry_id)
            .unwrap()
            .health
            .food
            .food_level = 19;
        assert!(can_start(&hungry.players[&hungry_id], &bread).is_some());
    }

    #[test]
    fn eating_advances_the_countdown_and_applies_food_and_remainder() {
        let (mut region, id) = region_with_item(items::SUSPICIOUS_STEW, 1);
        region.players.get_mut(&id).unwrap().health.food.food_level = 10;
        assert!(start(&mut region, id, Hand::Main));
        let using = region.players[&id].use_item.clone().unwrap();
        assert_eq!(using.total, 32);
        for _ in 0..31 {
            tick(&mut region);
        }
        assert!(region.players[&id].use_item.is_some());
        assert_eq!(region.players[&id].health.food.food_level, 10);
        tick(&mut region);
        assert!(region.players[&id].use_item.is_none());
        // 谜之炖菜吃 6 点、留一个碗。
        assert_eq!(region.players[&id].health.food.food_level, 16);
        assert_eq!(
            region.players[&id].inventory.held(Hand::Main).item,
            items::BOWL
        );
    }

    #[test]
    fn milk_clears_harmful_effects_and_leaves_a_bucket() {
        let (mut region, id) = region_with_item(items::MILK_BUCKET, 1);
        crate::player_effects::give(
            &mut region,
            id,
            mc_bamboo_blocks::entity::mob::effects::EffectInstance::with_amplifier(
                mob_effect::POISON,
                600,
                0,
            ),
        );
        crate::player_effects::give(
            &mut region,
            id,
            mc_bamboo_blocks::entity::mob::effects::EffectInstance::with_amplifier(
                mob_effect::SPEED,
                600,
                0,
            ),
        );
        assert!(start(&mut region, id, Hand::Main));
        for _ in 0..32 {
            tick(&mut region);
        }
        assert!(!region.players[&id].effects.has(mob_effect::POISON));
        assert!(region.players[&id].effects.has(mob_effect::SPEED));
        assert_eq!(
            region.players[&id].inventory.held(Hand::Main).item,
            items::BUCKET
        );
    }

    #[test]
    fn golden_apple_applies_its_official_effects() {
        let (mut region, id) = region_with_item(items::GOLDEN_APPLE, 1);
        assert!(start(&mut region, id, Hand::Main));
        for _ in 0..32 {
            tick(&mut region);
        }
        // 官方金苹果：伤害吸收 I + 再生 II。
        assert!(region.players[&id].effects.has(mob_effect::ABSORPTION));
        assert!(region.players[&id].effects.has(mob_effect::REGENERATION));
        assert_eq!(
            region.players[&id]
                .effects
                .amplifier(mob_effect::REGENERATION),
            Some(1)
        );
    }
}
