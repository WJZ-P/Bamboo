//! 流浪商人 `WanderingTrader` 的专属状态、`despawnDelay` 和饮水状态机。
//!
//! 交易层（`offers`、`inventory`、`trading_player`、`unhappy`）是 `AbstractVillager`
//! 的字段，和村民共用 [`super::Mob::villager`]；这里只放流浪商人自己的部分。
//!
//! 饮水照 `UseItemGoal` + `LivingEntity.updatingUsingItem` + `ItemStack.finishUsingItem`
//! 的时序：夜里（优先 0）开始喝隐身药水，白天（优先 0）开始喝牛奶；每刻减
//! `useItemRemaining`，途中按 `shouldEmitParticlesAndSounds` 每 4 刻用实体自己的
//! 随机源取 4 个数发 `OverrideConsumeSound` 的音效，喝完挂 / 清真正的状态效果。
//!
//! Wandering-trader-specific state, `despawnDelay` and the drinking state machine.
//! The trading layer is shared with villagers through `Mob::villager`. Drinking
//! follows `UseItemGoal` + `LivingEntity.updatingUsingItem` +
//! `ItemStack.finishUsingItem`: invisibility potion at night, milk by day, 32
//! ticks of use, per-4-tick `OverrideConsumeSound` draws from the entity RNG,
//! and real status effects when finished.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{
    potions,
    registries::{mob_effect, potion},
};

use super::{Mob, MobEvent, goals::GoalSelector, living, navigation::Navigation};
use crate::{
    RandomSource,
    entity::{Entity, EntityRandom},
};

/// `mob-oracle` 的改坏确认开关，只由 `--break-self-test` 驱动，其它驱动不调用。
/// 生产代码在对应路径读它，故意和官方分叉以证明对拍能抓到。
///
/// Break-test switches driven only by `mob-oracle`'s `--break-self-test`; no other
/// driver sets them. Production code reads the relevant flag to diverge from vanilla
/// on purpose, proving the oracle catches the regression.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum OracleTamper {
    #[default]
    None,
    /// `maybeDespawn` 每刻多扣一次倒计时（消失过早）。
    /// `maybeDespawn` decrements the counter twice per tick (despawns too soon).
    DespawnTooFast,
    /// `updateInvisibilityStatus` 不刷新共享隐身标志。
    /// `updateInvisibilityStatus` never refreshes the shared invisibility flag.
    NoInvisibilityRefresh,
    /// `notifyTrade` 不累加交易项使用次数。
    /// `notifyTrade` does not increment the offer's `uses`.
    NoTradeUses,
}

thread_local! {
    static ORACLE_TAMPER: std::cell::Cell<OracleTamper> =
        const { std::cell::Cell::new(OracleTamper::None) };
}

/// 由 `mob-oracle` 按 `--break-self-test` 设置，其它驱动不调用。
pub fn set_oracle_tamper(name: &str) {
    let tamper = match name {
        "wt_despawn" => OracleTamper::DespawnTooFast,
        "wt_invisibility" => OracleTamper::NoInvisibilityRefresh,
        "wt_trade" => OracleTamper::NoTradeUses,
        _ => OracleTamper::None,
    };
    ORACLE_TAMPER.with(|cell| cell.set(tamper));
}

pub(crate) fn oracle_tamper() -> OracleTamper {
    ORACLE_TAMPER.with(|cell| cell.get())
}

/// 刷新器生成流浪商人时设的 `despawnDelay`（`WanderingTraderSpawner.spawn`）。
///
/// The `despawnDelay` set by `WanderingTraderSpawner.spawn`.
pub const SPAWNED_DESPAWN_DELAY: i32 = 48000;

/// `Consumables.DEFAULT_DRINK` / `MILK_BUCKET` 的 `consumeSeconds = 1.6`，
/// 也就是 32 刻；`UseItemGoal.startUsingItem` 到 `completeUsingItem` 正好这段。
///
/// `Consumables.DEFAULT_DRINK` / `MILK_BUCKET` have `consumeSeconds = 1.6`
/// (32 ticks) between `UseItemGoal.startUsingItem` and `completeUsingItem`.
pub const DRINK_DURATION: i32 = 32;

/// `WanderingTrader.getConsumeSound`（`Consumable.OverrideConsumeSound`）。
///
/// `WanderingTrader.getConsumeSound` from `Consumable.OverrideConsumeSound`.
const DRINK_POTION_SOUND: &str = "minecraft:entity.wandering_trader.drink_potion";
const DRINK_MILK_SOUND: &str = "minecraft:entity.wandering_trader.drink_milk";
/// 两个 `UseItemGoal` 喝完的声音（`SoundEvents.WANDERING_TRADER_*`）。
const DISAPPEARED_SOUND: &str = "minecraft:entity.wandering_trader.disappeared";
const REAPPEARED_SOUND: &str = "minecraft:entity.wandering_trader.reappeared";

/// 当前正在使用的消耗品（官方 `LivingEntity.useItem` / `useItemRemaining`）。
///
/// The consumable currently in use (`LivingEntity.useItem` / `useItemRemaining`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Drink {
    #[default]
    None,
    /// 夜里喝隐身药水，喝完挂 `INVISIBILITY`。
    /// Drinking an invisibility potion at night; gains `INVISIBILITY` when done.
    Invisibility { remaining: i32 },
    /// 白天喝牛奶桶，喝完清掉所有状态效果。
    /// Drinking a milk bucket by day; clears all effects when done.
    Milk { remaining: i32 },
}

impl Drink {
    fn sound(self) -> &'static str {
        match self {
            Self::Milk { .. } => DRINK_MILK_SOUND,
            _ => DRINK_POTION_SOUND,
        }
    }
}

/// `WanderingTrader` 的专属状态。
///
/// The wandering trader's own state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WanderingTrader {
    /// `despawnDelay`：每刻 -1，到 0 时消失；交易中暂停递减。
    /// `despawnDelay`: ticked down each tick, despawning at zero; paused while trading.
    pub despawn_delay: i32,
    /// `wanderTarget`：`WanderToPositionGoal` 的目的地，刷新器设为村庄钟位置。
    /// `wanderTarget`: the goal's destination, set to the village bell by the spawner.
    pub wander_target: Option<BlockPos>,
    /// 当前饮水状态。
    /// The current drinking state.
    pub drink: Drink,
    /// `Entity.isInvisible()` 共享标志位：`updateInvisibilityStatus` 在
    /// `LivingEntity.baseTick` 里按上一刻的效果变化刷新，官方 `UseItemGoal`
    /// 的两个谓词读的是这个标志，不是效果表。
    ///
    /// The `Entity.isInvisible()` shared flag: `updateInvisibilityStatus`
    /// refreshes it in `LivingEntity.baseTick` from the previous tick's effect
    /// changes. The two `UseItemGoal` predicates read this flag, not the map.
    pub invisible: bool,
    /// `effectsDirty`：效果表变了，下一次 `baseTick` 才刷新共享标志位。
    pub effects_dirty: bool,
}

/// `WanderingTrader.registerGoals`：按官方 `addGoal` 顺序注册整张表。两条
/// `UseItemGoal`（隐身药水 / 牛奶）是真实目标，走 `GoalSelector` 的隔刻相位；
/// 七条 `AvoidEntityGoal` 按官方类名和距离注册，其中灾厄村民 / 亡灵未建模的
/// 恒匹配不到。`TradeWithPlayerGoal` / `LookAtTradingPlayerGoal` /
/// `WanderToPositionGoal` / `InteractGoal` 见各自实现。
///
/// `WanderingTrader.registerGoals`, in vanilla `addGoal` order. The two
/// `UseItemGoal`s (potion / milk) are real goals that follow the selector's
/// every-other-tick phase; the seven `AvoidEntityGoal`s keep the vanilla class
/// names and distances, the unmodelled illager / undead ones never matching.
pub(in crate::entity::mob) fn goals(navigation: &mut Navigation) -> GoalSelector {
    use super::goals::UseItemKind;
    // `FloatGoal` 构造函数打开导航浮水。
    // `FloatGoal`'s constructor enables floating on the navigation.
    navigation.flags.can_float = true;
    let avoid = |class: super::avoid::AvoidClass, max: f32| {
        super::goals::Goal::Avoid(super::avoid::AvoidEntity::new(class, max, 0.5, 0.5))
    };
    super::goals::GoalSelector::from_ordered_pairs(vec![
        (0, super::goals::Goal::Float),
        (
            0,
            super::goals::Goal::UseItem {
                kind: UseItemKind::Invisibility,
            },
        ),
        (
            0,
            super::goals::Goal::UseItem {
                kind: UseItemKind::Milk,
            },
        ),
        (1, super::goals::Goal::TradeWithPlayer),
        (1, avoid(super::avoid::AvoidClass::Zombie, 8.0)),
        (1, avoid(super::avoid::AvoidClass::Evoker, 12.0)),
        (1, avoid(super::avoid::AvoidClass::Vindicator, 8.0)),
        (1, avoid(super::avoid::AvoidClass::Vex, 8.0)),
        (1, avoid(super::avoid::AvoidClass::Pillager, 15.0)),
        (1, avoid(super::avoid::AvoidClass::Illusioner, 12.0)),
        (1, avoid(super::avoid::AvoidClass::Zoglin, 10.0)),
        (
            1,
            super::goals::Goal::Panic {
                speed: 0.5,
                target: mc_bamboo_core::Vec3::default(),
            },
        ),
        (
            1,
            super::goals::Goal::LookAtTradingPlayer {
                look_distance: 8.0,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            2,
            super::goals::Goal::WanderToPosition {
                stop_distance: 2.0,
                speed: 0.35,
            },
        ),
        (
            4,
            super::goals::Goal::MoveTowardsRestriction {
                speed: 0.35,
                wanted: mc_bamboo_core::Vec3::default(),
            },
        ),
        (
            8,
            super::goals::Goal::Stroll {
                speed: 0.35,
                interval: 120,
                probability: 0.001,
                wanted: mc_bamboo_core::Vec3::default(),
                force_trigger: false,
            },
        ),
        (
            9,
            super::goals::Goal::Interact {
                look_distance: 3.0,
                probability: 1.0,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            10,
            super::goals::Goal::LookAtMob {
                kind: None,
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
    ])
}

/// `LivingEntity.updateDirtyEffects` → `updateInvisibilityStatus`：效果表标脏后
/// 在 `baseTick` 里把 `isInvisible()` 同步成 `hasEffect(INVISIBILITY)`。
/// 官方 `addEffect` 标脏的那一刻共享标志位还没变，所以刚喝完的这一刻
/// `UseItemGoal` 仍会看到旧值。
///
/// `LivingEntity.updateDirtyEffects` → `updateInvisibilityStatus`: the shared
/// flag follows `hasEffect(INVISIBILITY)` in `baseTick`, one tick after the
/// effect map changed.
pub fn update_invisibility(mob: &mut Mob) {
    if oracle_tamper() == OracleTamper::NoInvisibilityRefresh {
        return;
    }
    if !mob.wandering_trader.effects_dirty {
        return;
    }
    mob.wandering_trader.effects_dirty = false;
    mob.wandering_trader.invisible = mob.effects.has(mob_effect::INVISIBILITY);
}

/// `WanderingTrader.maybeDespawn()`：`aiStep()` 末尾每刻跑一次。
///
/// `WanderingTrader.maybeDespawn()`, run once per tick at the end of `aiStep()`.
pub fn maybe_despawn(me: &mut Entity, mob: &mut Mob) {
    if mob.wandering_trader.despawn_delay > 0 && !mob.villager.is_trading() {
        let step = if oracle_tamper() == OracleTamper::DespawnTooFast {
            2
        } else {
            1
        };
        mob.wandering_trader.despawn_delay -= step;
        if mob.wandering_trader.despawn_delay == 0 {
            me.discard();
        }
    }
}

/// `AbstractVillager.notifyTrade` 对流浪商人的状态更新：累加对应交易项的使用次数，
/// 并把 `ambientSoundTime` 压回 `-getAmbientSoundInterval()`。经验球由调用方决定是否生成。
///
/// State update of `AbstractVillager.notifyTrade` for a wandering trader: increments
/// the offer's `uses` and resets `ambientSoundTime` to `-getAmbientSoundInterval()`.
pub fn notify_trade(mob: &mut Mob, offer_index: usize) {
    if oracle_tamper() != OracleTamper::NoTradeUses
        && let Some(offer) = mob
            .villager
            .offers
            .as_mut()
            .and_then(|offers| offers.get_mut(offer_index))
    {
        offer.uses += 1;
    }
    mob.ambient_sound_time = -mob.mob_type.ambient_sound_interval();
}

/// `ItemStack.onUseTick` 的 `Consumable.shouldEmitParticlesAndSounds`：
/// `consumeTicks - remaining > (int)(consumeTicks * 0.21875F)` 且
/// `remaining % 4 == 0`。32 刻 ⇒ 阈值 7，命中刻是 28 / 24 / … / 4 / 0。
///
/// `Consumable.shouldEmitParticlesAndSounds` from `ItemStack.onUseTick`.
fn should_emit_sound(remaining: i32) -> bool {
    let threshold = (f64::from(DRINK_DURATION) * 0.21875) as i32;
    DRINK_DURATION - remaining > threshold && remaining % 4 == 0
}

/// `Consumable.emitParticlesAndSounds`：实体随机源取 `nextBoolean`、
/// `triangle(1, 0.2)`（两次 `nextFloat`）、`Mth.randomBetween(0.9, 1)`（一次
/// `nextFloat`），DRINK 动画固定音量 0.5、音高最后那个值；音效名由
/// `OverrideConsumeSound.getConsumeSound` 覆盖。
///
/// `Consumable.emitParticlesAndSounds`: draw `nextBoolean`, `triangle(1, 0.2)`
/// (two `nextFloat`s) and `Mth.randomBetween(0.9, 1)` (one `nextFloat`) from
/// the entity RNG; DRINK uses volume 0.5 and that last value as pitch.
fn emit_drink_sound(me: &mut Entity, mob: &mut Mob, drink: Drink) {
    let sound = drink.sound();
    // `nextBoolean` / `triangle` / `randomBetween` 都走实体自己的随机源。
    let random = EntityRandom(std::cell::RefCell::new(me.random));
    let _pitch_scale = if random.next_boolean() { 0.5_f32 } else { 1.0 };
    let _triangle = 1.0_f32 + 0.2 * (random.next_float() - random.next_float());
    let pitch = 0.9 + random.next_float() * 0.1;
    me.random = random.0.into_inner();
    mob.events.push(MobEvent::Sound {
        sound,
        volume: 0.5,
        pitch,
        position: me.position,
        hostile: false,
        seed: None,
    });
}

/// `ItemStack.finishUsingItem` → `Consumable.onConsume`：
/// 隐身药水按 `PotionContents.forEachEffect` 挂 `INVISIBILITY` 3600 刻，
/// 牛奶按 `ClearAllStatusEffectsConsumeEffect` 清掉全部效果。
/// `emitParticlesAndSounds(..., 16)` 在 `onConsume` 开头再取 4 个数并放一声。
///
/// `ItemStack.finishUsingItem` → `Consumable.onConsume`: the potion adds
/// `INVISIBILITY` for 3600 ticks via `PotionContents.forEachEffect`; milk
/// clears all effects. `emitParticlesAndSounds(..., 16)` draws four more RNG
/// values and plays one more sound at the start of `onConsume`.
fn finish_drink(me: &mut Entity, mob: &mut Mob, drink: Drink) {
    emit_drink_sound(me, mob, drink);
    match drink {
        Drink::Invisibility { .. } => {
            let potion = potion::INVISIBILITY;
            for &(effect, duration, amplifier) in potions::effects(potion).unwrap_or(&[]) {
                let mut instance = super::effects::EffectInstance::new(effect, duration);
                instance.amplifier = amplifier;
                living::add_effect(mob, instance);
            }
        }
        Drink::Milk { .. } => {
            living::clear_effects(mob);
        }
        Drink::None => {}
    }
    // `onEffectAdded` / `removeAllEffects` 标脏，共享标志位下一刻 baseTick 才刷新。
    // `UseItemGoal.stop()` 由目标表在下一个完整刻调用，那里才取音高随机数；
    // `completeUsingItem` 这一刻只有 `Consumable.onConsume` 的 4 次取数。
    mob.wandering_trader.effects_dirty = true;
    mob.wandering_trader.drink = Drink::None;
}

/// `UseItemGoal.stop()` 的喝完音效：`playSound(finishUsingSound, 1.0, 0.9 + nextFloat * 0.2)`。
/// 音高在调 `playSound` 之前取，静音的生物也照样消耗这次随机数。
///
/// `UseItemGoal.stop()` plays the finish sound at volume 1.0 and pitch
/// `0.9 + nextFloat * 0.2`; the pitch draw happens before `playSound`, so a silent
/// mob still consumes it.
pub fn use_item_stop(me: &mut Entity, mob: &mut Mob, milk: bool) {
    let sound = if milk {
        REAPPEARED_SOUND
    } else {
        DISAPPEARED_SOUND
    };
    let random = EntityRandom(std::cell::RefCell::new(me.random));
    let pitch = 0.9 + random.next_float() * 0.2;
    me.random = random.0.into_inner();
    living::play_sound(me, mob, sound, 1.0, pitch);
}

/// `LivingEntity.updatingUsingItem()`：紧跟 `super.tick()`，在 `aiStep()` 之前。
/// `ItemStack.onUseTick` 先按 `remaining` 发音效（实体随机源取 4 个数），
/// `useItemRemaining` 减一，到 0 时 `completeUsingItem()` →
/// `finishUsingItem()` → `Consumable.onConsume()`。
///
/// `LivingEntity.updatingUsingItem()`: right after `super.tick()` and before
/// `aiStep()`; `onUseTick` emits the sound draws, then the remaining ticks
/// count down and `completeUsingItem()` applies the effects at zero.
pub fn use_tick(me: &mut Entity, mob: &mut Mob) {
    let drink = mob.wandering_trader.drink;
    let remaining = match drink {
        Drink::None => return,
        Drink::Invisibility { remaining } | Drink::Milk { remaining } => remaining,
    };
    if should_emit_sound(remaining) {
        emit_drink_sound(me, mob, drink);
    }
    if remaining <= 1 {
        finish_drink(me, mob, drink);
    } else {
        mob.wandering_trader.drink = match drink {
            Drink::Invisibility { .. } => Drink::Invisibility {
                remaining: remaining - 1,
            },
            Drink::Milk { .. } => Drink::Milk {
                remaining: remaining - 1,
            },
            Drink::None => Drink::None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::mob::MobType;

    fn trader() -> (crate::entity::Entity, Mob) {
        let kind = MobType::WanderingTrader;
        let mob = Mob::new(kind);

        (
            crate::entity::Entity::by_mob_type(kind, Default::default()),
            mob,
        )
    }

    #[test]
    fn despawn_delay_counts_down_and_discards_at_zero() {
        let (mut me, mut mob) = trader();
        mob.wandering_trader.despawn_delay = 3;
        maybe_despawn(&mut me, &mut mob);
        assert_eq!(mob.wandering_trader.despawn_delay, 2);
        assert!(!me.removed);
        maybe_despawn(&mut me, &mut mob);
        maybe_despawn(&mut me, &mut mob);
        assert_eq!(mob.wandering_trader.despawn_delay, 0);
        assert!(me.removed);
    }

    #[test]
    fn trading_pauses_the_despawn_delay() {
        let (mut me, mut mob) = trader();
        mob.wandering_trader.despawn_delay = 1;
        mob.villager.set_trading_player(Some(7));
        maybe_despawn(&mut me, &mut mob);
        assert_eq!(mob.wandering_trader.despawn_delay, 1);
        assert!(!me.removed);
    }

    #[test]
    fn drink_finish_applies_and_clears_effects() {
        let (mut me, mut mob) = trader();
        // 夜里启动隐身药水；喝完挂 3600 刻 INVISIBILITY。
        mob.wandering_trader.drink = Drink::Invisibility { remaining: 1 };
        finish_drink(&mut me, &mut mob, Drink::Invisibility { remaining: 1 });
        assert!(mob.effects.has(mob_effect::INVISIBILITY));
        assert_eq!(mob.wandering_trader.drink, Drink::None);
        // 效果表标脏，`updateInvisibilityStatus` 在同步阶段刷新共享标志位。
        assert!(mob.wandering_trader.effects_dirty);
        update_invisibility(&mut mob);
        assert!(mob.wandering_trader.invisible);
        // 白天启动牛奶；喝完清掉全部效果。
        mob.wandering_trader.drink = Drink::Milk { remaining: 1 };
        finish_drink(&mut me, &mut mob, Drink::Milk { remaining: 1 });
        assert!(!mob.effects.has(mob_effect::INVISIBILITY));
    }

    #[test]
    fn sound_emission_follows_the_official_remaining_ticks() {
        // 32 * 0.21875 = 7；只在 28..0 中 4 的倍数上发。
        assert!(should_emit_sound(24));
        assert!(should_emit_sound(4));
        assert!(!should_emit_sound(28));
        assert!(!should_emit_sound(30));
        assert!(!should_emit_sound(26));
    }
}
