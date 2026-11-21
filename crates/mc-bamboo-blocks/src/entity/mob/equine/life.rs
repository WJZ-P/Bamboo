//! 马科每刻的生活：`AbstractHorse.aiStep` / `tick` 里的甩尾、自然回血、吃草、
//! 扬蹄和张嘴计时，受伤扬蹄，`isImmobile`，以及按幼年分派的叫声。
//!
//! Equine per-tick life: tail swishes, natural healing, grazing, rearing and
//! mouth timers from `AbstractHorse.aiStep` / `tick`, rearing when hurt,
//! `isImmobile`, and the baby-aware voices.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, registries::attribute};

use super::{EquineState, Mob, MobType};
use crate::entity::{Entity, ctx::EntityCtx, mob::living};

fn state(mob: &mut Mob) -> &mut EquineState {
    mob.equine.as_mut().expect("equine state")
}

/// `AbstractHorse.isImmobile`：`(isDeadOrDying && isVehicle && isSaddled) || isEating || isStanding`。
/// 没有骑乘时第一项恒假，所以濒死的马照样跑 AI。羊驼覆写成 `isDeadOrDying || isEating`。
///
/// `AbstractHorse.isImmobile`. Without riding the first term is always false, so
/// a dying horse still runs its AI. The llama overrides it with `isDeadOrDying || isEating`.
pub(in crate::entity::mob) fn is_immobile(mob: &Mob) -> bool {
    let Some(state) = &mob.equine else {
        return mob.is_dead_or_dying();
    };
    if mob.mob_type == MobType::Llama {
        return mob.is_dead_or_dying() || state.eating;
    }
    state.eating || state.standing
}

/// `AbstractHorse.aiStep` 开头：`nextInt(200) == 0` 时甩尾。
///
/// The start of `AbstractHorse.aiStep`: swish the tail on `nextInt(200) == 0`.
pub(in crate::entity::mob) fn ai_step_start(me: &mut Entity, mob: &mut Mob) {
    if me.random.next_int_bounded(200) == 0 {
        state(mob).tail_counter = 1;
    }
}

/// `AbstractHorse.aiStep` 在 `super.aiStep()` 之后：活着时 `nextInt(900)` 回一点血，
/// 能吃草的（不是羊驼）不在吃、没人骑时 `nextInt(300)` 并且脚下是草方块就开始吃，
/// 吃满 50 刻停下。`followMommy` 只对存档里 `Bred` 为真的幼崽生效，而且官方只
/// `createPath` 不走，没有可见效果，这里不做。
///
/// After `super.aiStep()`: heal on `nextInt(900)`, start grazing on `nextInt(300)`
/// over a grass block, stop after 50 ticks. `followMommy` only computes a path it
/// never follows, so it has no visible effect and is skipped.
pub(in crate::entity::mob) fn ai_step_end(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !living::is_alive(me, mob) {
        return;
    }
    if me.random.next_int_bounded(900) == 0 && mob.death_time == 0 {
        heal(mob, 1.0);
    }
    if mob.mob_type == MobType::Llama {
        return;
    }
    let below = BlockPos::containing(me.position).below();
    if !state(mob).eating
        && me.random.next_int_bounded(300) == 0
        && ctx.blocks().block_state(below).is(blocks::GRASS_BLOCK)
    {
        set_eating(mob, true);
    }
    let state = state(mob);
    if state.eating {
        state.eating_counter += 1;
        if state.eating_counter > 50 {
            state.eating_counter = 0;
            state.eating = false;
        }
    }
}

/// `AbstractHorse.tick` 在 `super.tick()` 之后推进的三个服务端计时器。
/// `sprintCounter` 只有客户端会设，服务端恒为 0，不建模。
///
/// The three server-side timers `AbstractHorse.tick` advances after `super.tick()`.
/// `sprintCounter` is only set on the client and stays 0 here.
pub(in crate::entity::mob) fn tick_end(mob: &mut Mob) {
    let state = state(mob);
    if state.mouth_counter > 0 {
        state.mouth_counter += 1;
        if state.mouth_counter > 30 {
            state.mouth_counter = 0;
            state.mouth_open = false;
        }
    }
    if state.stand_counter > 0 {
        state.stand_counter -= 1;
        if state.stand_counter <= 0 {
            clear_standing(state);
        }
    }
    if state.tail_counter > 0 {
        state.tail_counter += 1;
        if state.tail_counter > 8 {
            state.tail_counter = 0;
        }
    }
}

/// `AbstractHorse.hurtServer`：真的受了伤，再 `nextInt(3) == 0` 时扬蹄。
///
/// `AbstractHorse.hurtServer`: after a real hit, rear on `nextInt(3) == 0`.
pub(in crate::entity::mob) fn after_hurt(me: &mut Entity, mob: &mut Mob) {
    if me.random.next_int_bounded(3) == 0 {
        stand_if_possible(mob);
    }
}

/// `LivingEntity.heal`：活着才加，`setHealth` 夹到最大血量。
///
/// `LivingEntity.heal`: only while alive; `setHealth` clamps to the maximum.
pub(in crate::entity::mob) fn heal(mob: &mut Mob, amount: f32) {
    if mob.health > 0.0 {
        let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
        mob.health = (mob.health + amount).clamp(0.0, max);
    }
}

fn set_eating(mob: &mut Mob, eating: bool) {
    state(mob).eating = eating;
}

fn clear_standing(state: &mut EquineState) {
    state.standing = false;
    state.stand_counter = 0;
}

/// `AbstractHorse.standIfPossible` → `setStanding(20)`；羊驼 `canPerformRearing` 为假。
///
/// `standIfPossible` → `setStanding(20)`; llamas cannot rear.
pub(in crate::entity::mob) fn stand_if_possible(mob: &mut Mob) {
    if mob.mob_type == MobType::Llama {
        return;
    }
    let state = state(mob);
    state.eating = false;
    state.standing = true;
    state.stand_counter = 20;
}

/// `AbstractHorse.openMouth`：张嘴 30 刻。
///
/// `AbstractHorse.openMouth`: keep the mouth open for 30 ticks.
pub(in crate::entity::mob) fn open_mouth(mob: &mut Mob) {
    let state = state(mob);
    state.mouth_counter = 1;
    state.mouth_open = true;
}

/// `AbstractHorse.makeMad`：没在扬蹄时扬蹄，再 `makeSound(getAngrySound())`
/// （声音音量、`getVoicePitch` 的两次 `nextFloat`）。
///
/// `makeMad`: rear unless already standing, then `makeSound(getAngrySound())`.
pub(in crate::entity::mob) fn make_mad(me: &mut Entity, mob: &mut Mob) {
    if state(mob).standing {
        return;
    }
    stand_if_possible(mob);
    let sound = voice(mob, Voice::Angry);
    living::make_sound(me, mob, sound);
}

/// 马科按种类和幼年分派的声音。
///
/// Equine sounds by kind and age.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::entity::mob) enum Voice {
    Ambient,
    Hurt,
    Death,
    Eat,
    Angry,
}

/// 只有马有幼年版本（`Horse.getAmbientSound` 等）。
///
/// Only the horse has baby variants (`Horse.getAmbientSound` and friends).
pub(in crate::entity::mob) fn voice(mob: &Mob, voice: Voice) -> &'static str {
    let baby = mob.is_baby();
    match (mob.mob_type, voice) {
        (MobType::Horse, Voice::Ambient) if baby => "minecraft:entity.baby_horse.ambient",
        (MobType::Horse, Voice::Ambient) => "minecraft:entity.horse.ambient",
        (MobType::Horse, Voice::Hurt) if baby => "minecraft:entity.baby_horse.hurt",
        (MobType::Horse, Voice::Hurt) => "minecraft:entity.horse.hurt",
        (MobType::Horse, Voice::Death) if baby => "minecraft:entity.baby_horse.death",
        (MobType::Horse, Voice::Death) => "minecraft:entity.horse.death",
        (MobType::Horse, Voice::Eat) if baby => "minecraft:entity.baby_horse.eat",
        (MobType::Horse, Voice::Eat) => "minecraft:entity.horse.eat",
        (MobType::Horse, Voice::Angry) if baby => "minecraft:entity.baby_horse.angry",
        (MobType::Horse, Voice::Angry) => "minecraft:entity.horse.angry",
        (MobType::Donkey, Voice::Ambient) => "minecraft:entity.donkey.ambient",
        (MobType::Donkey, Voice::Hurt) => "minecraft:entity.donkey.hurt",
        (MobType::Donkey, Voice::Death) => "minecraft:entity.donkey.death",
        (MobType::Donkey, Voice::Eat) => "minecraft:entity.donkey.eat",
        (MobType::Donkey, Voice::Angry) => "minecraft:entity.donkey.angry",
        (MobType::Mule, Voice::Ambient) => "minecraft:entity.mule.ambient",
        (MobType::Mule, Voice::Hurt) => "minecraft:entity.mule.hurt",
        (MobType::Mule, Voice::Death) => "minecraft:entity.mule.death",
        (MobType::Mule, Voice::Eat) => "minecraft:entity.mule.eat",
        (MobType::Mule, Voice::Angry) => "minecraft:entity.mule.angry",
        (_, Voice::Ambient) => "minecraft:entity.llama.ambient",
        (_, Voice::Hurt) => "minecraft:entity.llama.hurt",
        (_, Voice::Death) => "minecraft:entity.llama.death",
        (_, Voice::Eat) => "minecraft:entity.llama.eat",
        (_, Voice::Angry) => "minecraft:entity.llama.angry",
    }
}
