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
    entity::{Entity, EntityRandom, ctx::EntityCtx},
};

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
    /// `updatingUsingItem` 这一刻刚喝完：`UseItemGoal` 在这一刻的目标表里
    /// 还没 `stop()`，新的一次要等下一刻才 `canUse()`。
    pub finished_this_tick: bool,
    /// `UseItemGoal.stop()` 下一刻要播的喝完音效（音高在那一刻画一次随机数）。
    /// 0 = 没有，1 = 喝完这一刻设的（还不能播），2 = 下一刻可播；
    /// 3 / 4 同理但用牛奶的 `reappeared` 音效。
    pub finish_sound_ticks: u8,
}

/// `WanderingTrader.registerGoals`：官方用 `GoalSelector` 注册一串目标
/// （`UseItemGoal` 喝隐身药水 / 牛奶、`WanderToPositionGoal`、躲避僵尸等）。
/// 饮水由 [`custom_ai_step`] 按官方 `GoalSelector` / `updatingUsingItem` 的时序
/// 驱动，游走目标和消失走 [`wander_tick`] / [`maybe_despawn`]；目标表本身
/// 按注册顺序留一份供对拍读「正在跑的目标名」。
///
/// The official `WanderingTrader.registerGoals` uses a `GoalSelector`; drinking,
/// wandering and despawning are driven here by the per-tick steps, with the
/// registered goal list kept for the oracle's running-goal names.
pub(in crate::entity::mob) fn goals(_navigation: &mut Navigation) -> GoalSelector {
    super::goals::GoalSelector::from_ordered_pairs(vec![
        (0, super::goals::Goal::Float),
        (0, super::goals::Goal::Unavailable("UseItemGoal")),
        (0, super::goals::Goal::Unavailable("UseItemGoal")),
        (1, super::goals::Goal::Unavailable("TradeWithPlayerGoal")),
        (
            1,
            super::goals::Goal::Avoid(super::avoid::AvoidEntity::new(
                super::avoid::AvoidClass::Monster,
                8.0,
                0.5,
                0.5,
            )),
        ),
        (
            1,
            super::goals::Goal::Panic {
                speed: 0.5,
                target: mc_bamboo_core::Vec3::default(),
            },
        ),
        (
            1,
            super::goals::Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.0,
                look_at: None,
                look_time: 0,
            },
        ),
        (2, super::goals::Goal::Unavailable("WanderToPositionGoal")),
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
        (9, super::goals::Goal::Unavailable("InteractGoal")),
        (
            10,
            super::goals::Goal::LookAtPlayer {
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
        mob.wandering_trader.despawn_delay -= 1;
        if mob.wandering_trader.despawn_delay == 0 {
            me.discard();
        }
    }
}

/// `UseItemGoal.canUse` 的两个谓词，按官方注册顺序：夜里且没隐身 → 隐身药水；
/// 白天且隐身 → 牛奶。`isInvisible()` 由状态效果表派生（`updateInvisibilityStatus`）。
///
/// The two `UseItemGoal.canUse` predicates in registration order: dark and
/// visible → invisibility potion; bright and invisible → milk. `isInvisible()`
/// is derived from the effect map like `updateInvisibilityStatus`.
fn choose_drink(mob: &Mob, bright: bool) -> Option<Drink> {
    let invisible = mob.wandering_trader.invisible;
    if !bright && !invisible {
        Some(Drink::Invisibility {
            remaining: DRINK_DURATION,
        })
    } else if bright && invisible {
        Some(Drink::Milk {
            remaining: DRINK_DURATION,
        })
    } else {
        None
    }
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
    mob.wandering_trader.effects_dirty = true;
    mob.wandering_trader.finished_this_tick = true;
    // `UseItemGoal.stop()` 在目标表的下一次 tick 才跑，那时才取音高随机数；
    // `completeUsingItem` 这一刻只有 `Consumable.onConsume` 的 4 次取数。
    // `UseItemGoal.stop()` draws its pitch on the next goal tick, not here.
    mob.wandering_trader.drink = Drink::None;
    mob.wandering_trader.finish_sound_ticks = if matches!(drink, Drink::Milk { .. }) {
        3
    } else {
        1
    };
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

/// `UseItemGoal` 的两个 `canUse` 谓词：官方在 `Mob.serverAiStep()` 的目标表
/// 里跑，隔刻调度；这里在 `aiStep()` 尾部按注册顺序推进，并把正在跑的目标
/// 名写进目标表，供逐刻对拍读。
///
/// The two `UseItemGoal.canUse` predicates: vanilla runs them in the goal
/// selector inside `Mob.serverAiStep()`; here they advance at the `aiStep()`
/// tail and the running goal names go into the selector for the oracle.
pub fn custom_ai_step(_me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    // `Mob.serverAiStep` 在 `isEffectiveAi()` 为假（NoAI / 有人控制）时整个跳过，
    // 两个 `UseItemGoal` 也就不会启动；官方夹具的 NoAI 场景正是这样。
    if mob.no_ai {
        return;
    }
    let bright = ctx.blocks().is_bright_outside();
    // `UseItemGoal.stop()` 在目标表的下一次 tick 才把 MOVE 位放开；这一刻
    // `canUse()` 看到旧的 `isUsingItem()`，不会立刻开始新的一次。
    let finished = std::mem::take(&mut mob.wandering_trader.finished_this_tick);
    let ticks = mob.wandering_trader.finish_sound_ticks;
    let stop_sound = matches!(ticks, 2 | 4).then_some(if ticks == 4 {
        REAPPEARED_SOUND
    } else {
        DISAPPEARED_SOUND
    });
    if let Some(sound) = stop_sound {
        let random = EntityRandom(std::cell::RefCell::new(_me.random));
        let pitch = 0.9 + random.next_float() * 0.2;
        _me.random = random.0.into_inner();
        mob.events.push(MobEvent::Sound {
            sound,
            volume: 1.0,
            pitch,
            position: _me.position,
            hostile: false,
            seed: None,
        });
        mob.wandering_trader.finish_sound_ticks = 0;
    } else if ticks == 1 || ticks == 3 {
        mob.wandering_trader.finish_sound_ticks = ticks + 1;
    }
    if !finished
        && stop_sound.is_none()
        && let Some(next) = choose_drink(mob, bright)
        && mob.wandering_trader.drink == Drink::None
    {
        // `UseItemGoal.start()`：主手换成消耗品并 `startUsingItem`，这一刻不取随机数。
        mob.wandering_trader.drink = next;
    }
    // 把两个 `UseItemGoal` 的运行状态写回目标表（下标 1 / 2，按注册顺序）。
    // 喝完那一刻目标仍 running（`stop()` 在下一刻的目标表 tick 才被调用）。
    let which = match mob.wandering_trader.drink {
        Drink::Invisibility { .. } => Some(1),
        Drink::Milk { .. } => Some(2),
        Drink::None => finished.then_some(1),
    };
    for (index, wrapped) in mob.goals.goals.iter_mut().enumerate() {
        if matches!(wrapped.goal, super::goals::Goal::Unavailable("UseItemGoal")) {
            wrapped.running = which == Some(index);
        }
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
    fn drink_predicates_and_effect_application() {
        let (mut me, mut mob) = trader();
        // 夜里启动隐身药水；喝完挂 3600 刻 INVISIBILITY。
        mob.wandering_trader.drink = Drink::Invisibility { remaining: 1 };
        finish_drink(&mut me, &mut mob, Drink::Invisibility { remaining: 1 });
        assert!(mob.effects.has(mob_effect::INVISIBILITY));
        assert_eq!(mob.wandering_trader.drink, Drink::None);
        assert!(
            choose_drink(&mob, false).is_some(),
            "标志位还没刷新，仍会再喝一次"
        );
        update_invisibility(&mut mob);
        assert!(mob.wandering_trader.invisible);
        assert!(!choose_drink(&mob, false).is_some());
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
