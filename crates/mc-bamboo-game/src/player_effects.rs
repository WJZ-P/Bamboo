//! 玩家的效果：`LivingEntity.tickEffects` 的每刻推进、持续效果的落地、
//! `ServerPlayer.onEffectAdded/Updated/Removed` 的出站包，以及进服 / 重生的补发。
//!
//! 状态和计时都在 [`mc_bamboo_blocks::entity::mob::effects`] 里，玩家和生物共用。
//! 这里只把效果事件翻译成 `Region` 的改动和包字节，不改生物层的公式。
//!
//! Player effects: the per-tick advance of `LivingEntity.tickEffects`, the
//! continuous-effect actions, `ServerPlayer.onEffectAdded/Updated/Removed`
//! packets, and the re-sync after joining or respawning. The state lives in
//! the shared mob-effect module; this file only translates its events.

use mc_bamboo_blocks::entity::mob::{
    DamageKind, DamageSource,
    effects::{EffectAction, EffectChange, EffectInstance, EffectState},
};
use mc_bamboo_data::registries::mob_effect;
use mc_bamboo_protocol::packet::play::{RemoveMobEffect, UpdateMobEffect};

use crate::{inbound::PlayerId, player::Player, region::Region};

/// 一次施加效果的动作。`visible` / `ambient` / `show_icon` 由调用方填好。
///
/// One effect application; flags come from the caller.
pub(crate) fn give(region: &mut Region, id: PlayerId, instance: EffectInstance) -> bool {
    let (changed, actions) = give_in(&mut region.players, &mut region.links, id, instance);
    for tick in &actions {
        apply_action(region, id, tick.effect, tick.action);
    }
    changed
}

/// [`give`] 里只碰玩家表和连接的部分：加效果、发包、标脏、夹血量。
/// 瞬间效果的动作（治疗、伤害……）原样返回，交给拿得到整个 `Region` 的调用方。
/// 生物打中玩家时在精确驱动的实体循环里直接调它（尸壳的饥饿没有瞬间动作）。
pub(crate) fn give_in(
    players: &mut std::collections::BTreeMap<PlayerId, crate::player::Player>,
    links: &mut crate::links::Links,
    id: PlayerId,
    instance: EffectInstance,
) -> (
    bool,
    Vec<mc_bamboo_blocks::entity::mob::effects::EffectTick>,
) {
    let Some(player) = players.get_mut(&id) else {
        return (false, Vec::new());
    };
    let events = player.effects.add(instance);
    let changed = !events.changes.is_empty();
    send_changes_in(players, links, id, &events.changes);
    if let Some(player) = players.get_mut(&id) {
        if events.metadata_dirty {
            player.sync.metadata_dirty = true;
        }
        if events.attributes_changed {
            let max = max_health(player);
            player.health.health = player.health.health.min(max);
        }
    }
    (changed, events.actions)
}

/// 清一个或全部效果。
///
/// Removes one effect or all of them.
pub(crate) fn clear(region: &mut Region, id: PlayerId, effect: Option<i32>) -> bool {
    let Some(player) = region.players.get_mut(&id) else {
        return false;
    };
    let events = match effect {
        Some(effect) => player.effects.remove(effect),
        None => player.effects.remove_all(),
    };
    let changed = !events.changes.is_empty();
    send_changes(region, id, &events.changes);
    if events.metadata_dirty {
        mark_metadata_dirty(region, id);
    }
    if events.attributes_changed {
        refresh_limits(region, id);
    }
    changed
}

/// `ClearAllStatusEffectsConsumeEffect`（牛奶）：只清有害效果。
///
/// Milk's `ClearAllStatusEffectsConsumeEffect`: harmful effects only.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn clear_harmful(region: &mut Region, id: PlayerId) -> bool {
    let Some(player) = region.players.get_mut(&id) else {
        return false;
    };
    let events = player.effects.remove_harmful();
    let changed = !events.changes.is_empty();
    send_changes(region, id, &events.changes);
    if events.metadata_dirty {
        mark_metadata_dirty(region, id);
    }
    if events.attributes_changed {
        refresh_limits(region, id);
    }
    changed
}

/// `MobEffect.applyInstantaneousEffect`：药水 / consume_effects 的瞬间效果当场结算。
///
/// `MobEffect.applyInstantaneousEffect` for potions and consume effects.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn apply_instantaneous(region: &mut Region, id: PlayerId, effect: i32, amplifier: i32) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let events = player.effects.apply_instantaneous(effect, amplifier);
    for tick in &events.actions {
        apply_action(region, id, tick.effect, tick.action);
    }
}

/// `ServerPlayer.onEffectAdded/Updated/Removed` 和 `onEffectsRemoved` 的包。
/// 新增时 `blend = true`，更新时 `false`。
///
/// The `ServerPlayer` effect packets; additions blend, updates do not.
fn send_changes(region: &mut Region, id: PlayerId, changes: &[EffectChange]) {
    send_changes_in(&region.players, &mut region.links, id, changes);
}

fn send_changes_in(
    players: &std::collections::BTreeMap<PlayerId, crate::player::Player>,
    links: &mut crate::links::Links,
    id: PlayerId,
    changes: &[EffectChange],
) {
    for &change in changes {
        match change {
            EffectChange::Removed(effect) => {
                links.send(
                    id,
                    &RemoveMobEffect {
                        entity_id: id.0,
                        effect,
                    },
                );
            }
            EffectChange::Added(effect) | EffectChange::Updated(effect) => {
                let Some(instance) = players
                    .get(&id)
                    .and_then(|player| player.effects.get(effect))
                else {
                    continue;
                };
                links.send(
                    id,
                    &UpdateMobEffect {
                        entity_id: id.0,
                        effect,
                        amplifier: instance.amplifier,
                        duration: instance.duration,
                        ambient: instance.ambient,
                        visible: instance.visible,
                        show_icon: instance.show_icon,
                        blend: matches!(change, EffectChange::Added(_)),
                    },
                );
            }
        }
    }
}

/// `PlayerList.sendActivePlayerEffects`：进服和重生时按效果表顺序补发
/// 全部 `update_mob_effect`（`blend = false`）。
///
/// `PlayerList.sendActivePlayerEffects`: re-send every active effect after
/// joining or respawning, without blending.
pub(crate) fn send_active(region: &mut Region, id: PlayerId) {
    let Some(instances) = region
        .players
        .get(&id)
        .map(|player| player.effects.entries())
    else {
        return;
    };
    for instance in instances {
        region.links.send(
            id,
            &UpdateMobEffect {
                entity_id: id.0,
                effect: instance.effect,
                amplifier: instance.amplifier,
                duration: instance.duration,
                ambient: instance.ambient,
                visible: instance.visible,
                show_icon: instance.show_icon,
                blend: false,
            },
        );
    }
}

/// 每刻推进玩家的效果表：`LivingEntity.baseTick` 的 `tickEffects`。
/// 官方在 `Player.tick` 里、受伤冷却递减之后调用；这里接在 `player_hurt::tick`
/// 之后，治疗 / 伤害仍走同一个 `hurt_player` 入口。
///
/// One `LivingEntity.tickEffects` tick for every player. It runs after the
/// hurt-cooldown tick and routes healing/damage through `hurt_player`.
pub(crate) fn tick(region: &mut Region) {
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in ids {
        let Some(player) = region.players.get(&id) else {
            continue;
        };
        if player.dead || player.spawn_pending.is_some() {
            continue;
        }
        let state = EffectState {
            health: player.health.health,
            max_health: 20.0,
            absorption: player.health.absorption,
        };
        let Some(player) = region.players.get_mut(&id) else {
            continue;
        };
        let events = player.effects.tick(player.entity_ticks, state);
        send_changes(region, id, &events.changes);
        for tick in &events.actions {
            apply_action(region, id, tick.effect, tick.action);
        }
        if events.metadata_dirty {
            mark_metadata_dirty(region, id);
        }
        if events.attributes_changed {
            refresh_limits(region, id);
        }
    }
}

/// 把一刻度效果动作写进玩家状态。伤害走完整的 `hurt_player` 链路。
///
/// Applies one tick's effect actions. Damage goes through the full hurt path.
fn apply_action(region: &mut Region, id: PlayerId, effect: i32, action: EffectAction) {
    match action {
        EffectAction::Heal(amount) | EffectAction::InstantHeal(amount) => {
            if let Some(player) = region.players.get_mut(&id) {
                let health = player.health.health;
                player.health.health = (health + amount).min(max_health(player));
            }
        }
        EffectAction::MagicDamage(amount) => {
            // `PoisonMobEffect`：血量高于 1 才真的打一次。
            let alive = region
                .players
                .get(&id)
                .is_some_and(|player| player.health.health > 1.0);
            if alive {
                hurt(region, id, DamageKind::Magic, amount);
            }
        }
        EffectAction::WitherDamage(amount) => hurt(region, id, DamageKind::Wither, amount),
        EffectAction::InstantDamage(amount) => hurt(region, id, DamageKind::Magic, amount),
        EffectAction::Exhaustion(amount) => {
            if let Some(player) = region.players.get_mut(&id) {
                player.health.food.exhaustion = (player.health.food.exhaustion + amount).min(40.0);
            }
        }
        EffectAction::Eat {
            nutrition,
            saturation,
        } => {
            eat(region, id, nutrition, saturation);
        }
        EffectAction::Absorption(amount) => {
            if let Some(player) = region.players.get_mut(&id) {
                player.health.absorption = player.health.absorption.max(amount);
            }
        }
        EffectAction::AbsorptionEnded => {
            clear(region, id, Some(effect));
        }
    }
}

/// `FoodData.eat(int, float)`（饱和效果走这条）：
/// `food = min(food + n, 20)`，`saturation = min(saturation + n * s * 2, food_level)`。
pub(crate) fn eat(region: &mut Region, id: PlayerId, nutrition: i32, saturation: f32) {
    add_food(
        region,
        id,
        nutrition,
        nutrition.max(0) as f32 * saturation * 2.0,
    );
}

/// `FoodData.eat(FoodProperties)`：物品组件的 `saturation` 已经是
/// `nutrition * modifier * 2`，直接传给 `add`，不要再乘。
///
/// The item component's saturation is already `nutrition * modifier * 2`.
pub(crate) fn eat_food(region: &mut Region, id: PlayerId, nutrition: i32, saturation: f32) {
    add_food(region, id, nutrition, saturation);
}

fn add_food(region: &mut Region, id: PlayerId, nutrition: i32, saturation: f32) {
    if let Some(player) = region.players.get_mut(&id) {
        let food = &mut player.health.food;
        food.food_level = (food.food_level + nutrition.max(0)).clamp(0, 20);
        food.saturation = (food.saturation + saturation).clamp(0.0, food.food_level as f32);
    }
}

fn hurt(region: &mut Region, id: PlayerId, kind: DamageKind, amount: f32) {
    crate::player_hurt::hurt_player(
        region,
        id,
        DamageSource::environment(kind),
        amount,
        &mut None,
    );
}

/// `Attributes.MAX_HEALTH` 加生命提升效果：`20 + 4*(amplifier+1)`。
///
/// `Attributes.MAX_HEALTH` with health boost.
pub(crate) fn max_health(player: &Player) -> f32 {
    let base = 20.0_f32;
    let boost = player
        .effects
        .amplifier(mob_effect::HEALTH_BOOST)
        .map_or(0.0, |amplifier| {
            4.0 * (amplifier.clamp(0, 255) as f32 + 1.0)
        });
    base + boost
}

fn mark_metadata_dirty(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id) {
        player.sync.metadata_dirty = true;
    }
}

/// 按效果表夹住血量 / 吸收的上限（`onEffect*` 的 `refreshDirtyAttributes`）。
/// 生命提升消失时血量随最大血量降下来；吸收效果消失时吸收血量不立刻清零，
/// 由 `AbsorptionMobEffect.applyEffectTick` 的到期分支处理。
///
/// Clamps health and absorption after the effect table changes.
pub(crate) fn refresh_limits(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id) {
        let max = max_health(player);
        player.health.health = player.health.health.min(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    fn region_with_player() -> (Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let player = Player::new(
            id,
            Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::default(),
        );
        region.insert_player(player);
        (region, id)
    }

    #[test]
    fn milk_only_clears_harmful_effects_and_instant_effects_apply_at_once() {
        let (mut region, id) = region_with_player();
        give(
            &mut region,
            id,
            EffectInstance::with_amplifier(mob_effect::SPEED, 100, 0),
        );
        give(
            &mut region,
            id,
            EffectInstance::with_amplifier(mob_effect::POISON, 100, 0),
        );
        assert!(clear_harmful(&mut region, id));
        assert!(region.players[&id].effects.has(mob_effect::SPEED));
        assert!(!region.players[&id].effects.has(mob_effect::POISON));
        let hp = region.players[&id].health.health;
        apply_instantaneous(&mut region, id, mob_effect::INSTANT_DAMAGE, 0);
        assert_eq!(region.players[&id].health.health, hp - 6.0);
        // 瞬间效果不进表，只有牛奶留下的有益效果还在。
        assert_eq!(region.players[&id].effects.len(), 1);
        assert!(region.players[&id].effects.has(mob_effect::SPEED));
    }
}
