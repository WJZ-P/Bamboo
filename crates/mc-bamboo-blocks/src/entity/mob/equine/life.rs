//! 马科每刻的生活：`AbstractHorse.aiStep` / `tick` 里的甩尾、自然回血、吃草、
//! 扬蹄和张嘴计时，受伤扬蹄，`isImmobile`，以及按幼年分派的叫声。
//!
//! Equine per-tick life: tail swishes, natural healing, grazing, rearing and
//! mouth timers from `AbstractHorse.aiStep` / `tick`, rearing when hurt,
//! `isImmobile`, and the baby-aware voices.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, registries::attribute};

use super::{EquineState, Mob, MobType};
use crate::entity::{Entity, ctx::BlockView, ctx::EntityCtx, mob::living};

fn state(mob: &mut Mob) -> &mut EquineState {
    mob.equine.as_mut().expect("equine state")
}

/// `AbstractHorse.isSaddled()`：鞍位上有东西。
///
/// `AbstractHorse.isSaddled()`: the saddle slot is occupied.
pub(in crate::entity::mob) fn is_saddled(mob: &Mob) -> bool {
    !mob.equipment
        .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Saddle)
        .is_empty()
}

/// `AbstractHorse.isImmobile`：`(isDeadOrDying && isVehicle && isSaddled) || isEating || isStanding`。
/// 羊驼覆写成 `isDeadOrDying || isEating`。
///
/// `AbstractHorse.isImmobile`: `(isDeadOrDying && isVehicle && isSaddled) || isEating
/// || isStanding`. The llama overrides it with `isDeadOrDying || isEating`.
pub(in crate::entity::mob) fn is_immobile(mob: &Mob) -> bool {
    let Some(state) = &mob.equine else {
        return mob.is_dead_or_dying();
    };
    if mob.mob_type == MobType::Llama {
        return mob.is_dead_or_dying() || state.eating;
    }
    (mob.is_dead_or_dying() && mob.is_vehicle() && is_saddled(mob))
        || state.eating
        || state.standing
}

/// `AbstractHorse.tickRidden`：朝向跟着骑手（俯仰取一半），旧朝向、身体和头都对齐到新朝向。
/// 服务端上玩家控制的载具 `isLocalInstanceAuthoritative()` 为假，所以跳跃蓄力那一段不跑，
/// 起跳由客户端模拟后经 `move_vehicle` 报位置。
///
/// `AbstractHorse.tickRidden`: the facing follows the rider (half the pitch), and the
/// previous yaw, body and head all snap to the new yaw. On the server a player
/// controlled vehicle is not locally authoritative, so the jump-charge branch does
/// not run: the client simulates the jump and reports the position with
/// `move_vehicle`.
pub(in crate::entity::mob) fn tick_ridden(me: &mut Entity, mob: &mut Mob, yaw: f32, pitch: f32) {
    let yaw = if yaw.is_finite() { yaw } else { me.y_rot };
    me.y_rot = yaw;
    mob.x_rot = crate::entity::mob::control::clamp_x_rot(pitch * 0.5, mob.x_rot);
    mob.y_rot_o = yaw;
    mob.y_body_rot = yaw;
    mob.y_head_rot = yaw;
    mob.y_head_rot_o = yaw;
    mob.y_body_rot_o = yaw;
}

/// `AbstractHorse.causeFallDamage` 开头：摔落超过 1 格时用 0.4 音量、1.0 音高播落地声，
/// 不取随机数。羊驼覆写了整个方法，不播这个声音。伤害公式和 `LivingEntity` 的相同，
/// 由调用方照常结算。
///
/// The start of `AbstractHorse.causeFallDamage`: falls longer than one block play
/// the landing sound at volume 0.4 and pitch 1.0 without drawing randomness. The
/// llama overrides the whole method and stays quiet. The damage formula is the
/// same as `LivingEntity`'s and the caller settles it as usual.
pub(in crate::entity::mob) fn land_sound(me: &Entity, mob: &mut Mob, distance: f64) {
    if mob.mob_type == MobType::Llama || distance <= 1.0 {
        return;
    }
    let sound = if mob.is_baby() {
        "minecraft:entity.baby_horse.land"
    } else {
        "minecraft:entity.horse.land"
    };
    living::play_sound(me, mob, sound, 0.4, 1.0);
}

/// `AbstractHorse.playStepSound` / `Llama.playStepSound`：音量和音高取自脚下的方块
/// 声音类型（上面盖着雪层时换成雪的声音），被骑着跑的马按 `gallopSoundCounter`
/// 换成奔跑步声；每次奔跑步声后再抽签决定是否喷气。
///
/// `AbstractHorse.playStepSound` / `Llama.playStepSound`: volume and pitch come
/// from the sound type of the block below (a snow layer above wins), and a ridden
/// galloping horse switches to gallop sounds, each followed by a breath roll.
pub(in crate::entity::mob) fn play_step_sound(
    me: &mut Entity,
    mob: &mut Mob,
    view: &dyn BlockView,
    pos: BlockPos,
    block_state: crate::state::BlockState,
) {
    if mob.mob_type == MobType::Llama {
        living::play_sound(me, mob, "minecraft:entity.llama.step", 0.15, 1.0);
        return;
    }
    if block_state.is_liquid() {
        return;
    }
    let mut sound_type = block_state.sound_type();
    let above = view.block_state(pos.above());
    if above.is(blocks::SNOW) {
        sound_type = above.sound_type();
    }
    let volume = sound_type.volume * 0.15;
    let pitch = sound_type.pitch;
    // `canGallop`：只有马为真（驴、骡走 `AbstractChestedHorse` 的假）。
    //
    // `canGallop` is true for horses only; donkeys and mules come from
    // `AbstractChestedHorse`, which turns it off.
    if mob.is_vehicle() && mob.mob_type == MobType::Horse {
        let counter = &mut state(mob).gallop_sound_counter;
        *counter = counter.wrapping_add(1);
        if *counter > 5 && *counter % 3 == 0 {
            play_gallop_sound(me, mob, sound_type);
        } else if *counter <= 5 {
            living::play_sound(me, mob, "minecraft:entity.horse.step_wood", volume, pitch);
        }
        return;
    }
    if is_wood_sound_type(sound_type) {
        living::play_sound(me, mob, "minecraft:entity.horse.step_wood", volume, pitch);
    } else {
        let sound = if mob.is_baby() {
            "minecraft:entity.baby_horse.step"
        } else {
            "minecraft:entity.horse.step"
        };
        living::play_sound(me, mob, sound, volume, pitch);
    }
}

/// `Horse.playGallopSound`：先播奔跑步声，再用实体随机源判定喷气声。
///
/// `Horse.playGallopSound`: play the gallop first, then roll for a breath on the entity RNG.
fn play_gallop_sound(me: &mut Entity, mob: &mut Mob, sound: &mc_bamboo_data::SoundTypeDef) {
    living::play_sound(
        me,
        mob,
        "minecraft:entity.horse.gallop",
        sound.volume * 0.15,
        sound.pitch,
    );
    if me.random.next_int_bounded(10) == 0 {
        let breath = if mob.is_baby() {
            "minecraft:entity.baby_horse.breathe"
        } else {
            "minecraft:entity.horse.breathe"
        };
        living::play_sound(me, mob, breath, sound.volume * 0.6, sound.pitch);
    }
}

/// `AbstractHorse.isWoodSoundType`：五种木头声音类型。
///
/// `AbstractHorse.isWoodSoundType`: the five wooden sound types.
fn is_wood_sound_type(sound: &mc_bamboo_data::SoundTypeDef) -> bool {
    [
        blocks::OAK_PLANKS,
        blocks::CRIMSON_PLANKS,
        blocks::CRIMSON_STEM,
        blocks::CHERRY_PLANKS,
        blocks::BAMBOO_PLANKS,
    ]
    .into_iter()
    .any(|state| std::ptr::eq(crate::state::BlockState(state).sound_type(), sound))
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
/// 吃满 50 刻停下，最后 `followMommy`。
///
/// After `super.aiStep()`: heal on `nextInt(900)`, start grazing on `nextInt(300)`
/// over a grass block, stop after 50 ticks, then `followMommy`.
pub(in crate::entity::mob) fn ai_step_end(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !living::is_alive(me, mob) {
        return;
    }
    if me.random.next_int_bounded(900) == 0 && mob.death_time == 0 {
        heal(mob, 1.0);
    }
    if mob.mob_type != MobType::Llama {
        graze(me, mob, ctx);
    }
    follow_mommy(me, mob, ctx);
}

/// `AbstractHorse.aiStep` 里 `canEatGrass()` 为真（不是羊驼）的那一段。有人骑着时
/// 官方在取 `nextInt(300)` 之前就短路，随机源不会多走一步。
///
/// The `canEatGrass()` part of `AbstractHorse.aiStep` (everything but llamas). Vanilla
/// short-circuits before drawing `nextInt(300)` while someone rides, so the random
/// source does not advance.
fn graze(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let below = BlockPos::containing(me.position).below();
    let eating = state(mob).eating;
    if !eating
        && !mob.is_vehicle()
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

/// `AbstractHorse.followMommy`（羊驼不在商队时同样）：繁殖出生、还是幼崽、没在吃草时，
/// 在碰撞箱外扩 16 格里找 16 格内最近的、活着的、`isBred()` 的马科，离得超过 2 格就
/// `navigation.createPath(mommy, 0)`。官方只算路径不走，但会改导航的目标格并重置卡住计时。
///
/// `AbstractHorse.followMommy` (llamas outside a caravan alike): a bred, baby,
/// non-grazing equine looks for the nearest living bred equine within 16 blocks
/// and, if it is more than 2 blocks away, calls `navigation.createPath(mommy, 0)`.
/// Vanilla never follows that path, but it updates the target and resets the
/// stuck timeout.
fn follow_mommy(me: &Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let (bred, eating) = {
        let state = state(mob);
        (state.bred, state.eating)
    };
    if !bred || !mob.is_baby() || eating {
        return;
    }
    let area = me.bounding_box().inflate(16.0, 16.0, 16.0);
    let mut mommy: Option<(f64, mc_bamboo_core::Vec3)> = None;
    ctx.for_each_other_in(area, &mut |other| {
        let crate::entity::KindPublic::Mob(public) = &other.kind else {
            return;
        };
        let bred = public.equine.is_some_and(|equine| equine.flags & 8 != 0);
        if other.removed || !public.is_alive() || !bred {
            return;
        }
        let distance = me.position.distance_to_sqr(other.position);
        if distance <= 16.0 * 16.0 && mommy.is_none_or(|(best, _)| distance < best) {
            mommy = Some((distance, other.position));
        }
    });
    let Some((distance, position)) = mommy else {
        return;
    };
    if distance > 4.0 {
        let nav = living::nav_mob(me, mob, ctx.blocks());
        mob.navigation
            .create_path(ctx.blocks(), &nav, BlockPos::containing(position), 0);
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
    // `AbstractHorse.tick`：推进 `standAnimO` / `standAnim`，用于骑手附着点偏移。
    // `AbstractHorse.tick`: advance `standAnimO` / `standAnim` for the rider attachment offset.
    state.stand_anim_o = state.stand_anim;
    if state.standing {
        state.stand_anim += (1.0 - state.stand_anim) * 0.4 + 0.05;
        if state.stand_anim > 1.0 {
            state.stand_anim = 1.0;
        }
    } else {
        state.stand_anim +=
            (0.8 * state.stand_anim * state.stand_anim * state.stand_anim - state.stand_anim) * 0.6
                - 0.05;
        if state.stand_anim < 0.0 {
            state.stand_anim = 0.0;
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

/// `AbstractHorse.handleStartJump`：允许站在鞍上滑行、扬蹄，再播跳跃声。
/// `handleStopJump` 是空的，所以落地那一边不用做什么。
///
/// `AbstractHorse.handleStartJump`: allow standing on the saddle, rear, then play
/// the jump sound. `handleStopJump` is empty, so the landing side does nothing.
pub(in crate::entity::mob) fn handle_start_jump(me: &mut Entity, mob: &mut Mob) {
    stand_if_possible(mob);
    living::play_sound(me, mob, "minecraft:entity.horse.jump", 0.4, 1.0);
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

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;

    use super::*;
    use crate::entity::mob::MobEvent;
    use crate::state::BlockState;

    fn landing_sounds(kind: MobType, baby: bool, distance: f64) -> Vec<&'static str> {
        let mut entity = Entity::by_mob_type(kind, Vec3::new(0.5, 64.0, 0.5));
        entity
            .with_mob(|me, mob| {
                if baby {
                    mob.age = -24000;
                }
                land_sound(me, mob, distance);
                mob.events
                    .iter()
                    .filter_map(|event| match event {
                        MobEvent::Sound { sound, .. } => Some(*sound),
                        _ => None,
                    })
                    .collect()
            })
            .expect("equine row")
    }

    /// 马、驴、骡摔落超过 1 格播落地声（幼崽换幼马的声音），羊驼不播。
    ///
    /// Horses, donkeys and mules play the landing sound past one block (babies use
    /// the foal sound); llamas stay quiet.
    #[test]
    fn landing_sound_follows_the_abstract_horse_override() {
        assert!(landing_sounds(MobType::Horse, false, 1.0).is_empty());
        assert_eq!(
            landing_sounds(MobType::Horse, false, 1.5),
            ["minecraft:entity.horse.land"]
        );
        assert_eq!(
            landing_sounds(MobType::Donkey, true, 3.0),
            ["minecraft:entity.baby_horse.land"]
        );
        assert_eq!(
            landing_sounds(MobType::Mule, false, 3.0),
            ["minecraft:entity.horse.land"]
        );
        assert!(landing_sounds(MobType::Llama, false, 9.0).is_empty());
    }

    /// `START_RIDING_JUMP` 的服务端门槛：官方 `handlePlayerCommand` 只在
    /// `canJump()`（上鞍）且 `data > 0` 时调 `handleStartJump`（扬蹄 + 跳跃声）。
    /// 蓄力值本身由客户端计算冲量，服务端只用它判正数，所以不随刻度改变行为。
    ///
    /// Server gate for `START_RIDING_JUMP`: vanilla `handlePlayerCommand` calls
    /// `handleStartJump` (rear + jump sound) only when `canJump()` (saddled) and
    /// `data > 0`. The charge drives the client-side impulse; the server only
    /// checks that it is positive, so the behaviour does not vary with the scale.
    #[test]
    fn riding_jump_needs_a_saddle_and_a_positive_scale() {
        use mc_bamboo_data::items;
        use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

        let mut entity = Entity::by_mob_type(MobType::Horse, Vec3::new(0.5, 64.0, 0.5));
        entity
            .with_mob(|me, mob| {
                mob.events.clear();
                // 没上鞍时任意刻度都不响应（`canJump()` 为假）。
                // Without a saddle no scale reacts (`canJump()` is false).
                for scale in [0, -5, 100] {
                    assert!(!crate::entity::mob::start_riding_jump(me, mob, scale));
                }
                assert!(!state(mob).standing);
                assert!(mob.events.is_empty());

                // 上鞍后只有正刻度才扬蹄 20 刻、清掉吃草、播一次跳跃声。
                // Once saddled, only a positive scale rears for 20 ticks, clears
                // grazing and plays the jump sound once.
                mob.equipment
                    .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
                state(mob).eating = true;
                assert!(!crate::entity::mob::start_riding_jump(me, mob, 0));
                assert!(!state(mob).standing);
                assert!(crate::entity::mob::start_riding_jump(me, mob, 90));
                {
                    let state = state(mob);
                    assert!(state.standing);
                    assert_eq!(state.stand_counter, 20);
                    assert!(!state.eating);
                }
                let sounds: Vec<&'static str> = mob
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        MobEvent::Sound { sound, .. } => Some(*sound),
                        _ => None,
                    })
                    .collect();
                assert_eq!(sounds, ["minecraft:entity.horse.jump"]);
            })
            .expect("horse row");
    }

    /// 繁殖标记的幼崽找繁殖标记的成年马科算一条路（只改导航目标，不走）。
    fn mommy_target(adult_bred: bool, foal_kind: MobType) -> Option<BlockPos> {
        let mut level = crate::testing::TestLevel::new();
        for x in -2..12 {
            for z in -3..4 {
                level.set(BlockPos::new(x, 63, z), crate::BlockState(blocks::STONE));
            }
        }
        let mut ticks = crate::BlockTicks::default();
        let mut world = crate::World::new(&mut level, &mut ticks);
        let mut adult = Entity::by_mob_type(MobType::Donkey, Vec3::new(6.5, 64.0, 0.5));
        adult.with_mob(|_, mob| state(mob).bred = adult_bred);
        world.spawn_entity(adult);
        let mut foal = Entity::by_mob_type(foal_kind, Vec3::new(0.5, 64.0, 0.5));
        foal.on_ground = true;
        foal.with_mob(|me, mob| {
            mob.age = -24000;
            state(mob).bred = true;
            let mut ctx = crate::entity::ctx::PreciseCtx::new(&mut world, 0);
            follow_mommy(me, mob, &mut ctx);
            mob.navigation.target_pos
        })
        .expect("equine row")
    }

    /// `followMommy` 只认 `isBred()` 的马科，羊驼幼崽也找（不在商队）。
    ///
    /// `followMommy` only picks bred equines; llama foals look too outside a caravan.
    #[test]
    fn bred_foal_plans_a_path_to_the_nearest_bred_equine() {
        assert_eq!(
            mommy_target(true, MobType::Horse),
            Some(BlockPos::new(6, 64, 0))
        );
        assert_eq!(
            mommy_target(true, MobType::Llama),
            Some(BlockPos::new(6, 64, 0))
        );
        assert_eq!(mommy_target(false, MobType::Horse), None);
    }

    /// 在某个方块上走一步的声音：`(声音, 音量, 音高)` 列表。
    ///
    /// One step on a block: the list of `(sound, volume, pitch)`.
    fn step_sounds(
        kind: MobType,
        baby: bool,
        ridden: bool,
        ground: BlockState,
        steps: usize,
    ) -> Vec<(&'static str, f32, f32)> {
        let pos = BlockPos::new(0, 63, 0);
        let mut level = crate::testing::TestLevel::new();
        level.set(pos, ground);
        let mut ticks = crate::BlockTicks::new();
        let world = crate::World::new(&mut level, &mut ticks);
        let mut entity = Entity::by_mob_type(kind, Vec3::new(0.5, 64.0, 0.5));
        entity
            .with_mob(|me, mob| {
                me.random = crate::entity::JavaRandom::new(0);
                if baby {
                    mob.age = -24000;
                }
                if ridden {
                    mob.passengers.push(7);
                }
                for _ in 0..steps {
                    play_step_sound(me, mob, &world, pos, ground);
                }
                mob.events
                    .iter()
                    .filter_map(|event| match event {
                        MobEvent::Sound {
                            sound,
                            volume,
                            pitch,
                            ..
                        } => Some((*sound, *volume, *pitch)),
                        _ => None,
                    })
                    .collect()
            })
            .expect("equine row")
    }

    /// `AbstractHorse.playStepSound`：草地用马的脚步声，木头用地板声（驴、骡也是），
    /// 幼马用幼马的脚步声，羊驼用自己的脚步声。
    ///
    /// `AbstractHorse.playStepSound`: grass uses the horse step, wood uses the
    /// wooden step (donkeys and mules too), foals use the baby step and llamas
    /// their own.
    #[test]
    fn step_sounds_follow_the_abstract_horse_override() {
        let grass = BlockState(blocks::GRASS_BLOCK);
        let sound = grass.sound_type();
        let expected = (
            "minecraft:entity.horse.step",
            sound.volume * 0.15,
            sound.pitch,
        );
        assert_eq!(
            step_sounds(MobType::Horse, false, false, grass, 1),
            [expected]
        );
        assert_eq!(
            step_sounds(MobType::Horse, true, false, grass, 1)
                .first()
                .map(|sound| sound.0),
            Some("minecraft:entity.baby_horse.step")
        );
        assert_eq!(
            step_sounds(
                MobType::Donkey,
                false,
                false,
                BlockState(blocks::OAK_PLANKS),
                1
            )
            .first()
            .map(|sound| sound.0),
            Some("minecraft:entity.horse.step_wood")
        );
        assert_eq!(
            step_sounds(MobType::Llama, false, false, grass, 1),
            [("minecraft:entity.llama.step", 0.15, 1.0)]
        );
        // `blockState.liquid()`：水里不播。
        assert!(step_sounds(MobType::Horse, false, false, BlockState::WATER, 1).is_empty());
    }

    /// 被骑着的马：前五步是木头地板声，第六步起每三步一次奔跑步声，其余步不播。
    ///
    /// A ridden horse: the first five steps are wooden, from the sixth on every
    /// third step gallops and the rest stay silent.
    #[test]
    fn a_ridden_horse_gallops_every_third_step() {
        let plank = BlockState(blocks::OAK_PLANKS);
        let sounds: Vec<&'static str> = step_sounds(MobType::Horse, false, true, plank, 9)
            .into_iter()
            .map(|sound| sound.0)
            .collect();
        assert_eq!(
            sounds,
            [
                "minecraft:entity.horse.step_wood",
                "minecraft:entity.horse.step_wood",
                "minecraft:entity.horse.step_wood",
                "minecraft:entity.horse.step_wood",
                "minecraft:entity.horse.step_wood",
                "minecraft:entity.horse.gallop",
                "minecraft:entity.horse.breathe",
                "minecraft:entity.horse.gallop",
            ]
        );
    }

    #[test]
    fn a_gallop_rolls_once_even_when_the_foal_is_silent() {
        let ground = BlockState(blocks::GRASS_BLOCK);
        let sound_type = ground.sound_type();
        let mut entity = Entity::by_mob_type(MobType::Horse, Vec3::new(0.5, 64.0, 0.5));
        entity.with_mob(|me, mob| {
            mob.age = -24000;
            me.random = crate::entity::JavaRandom::new(0);
            let mut expected_rng = me.random;
            assert_eq!(expected_rng.next_int_bounded(10), 0);
            play_gallop_sound(me, mob, sound_type);
            assert_eq!(me.random, expected_rng);
            assert!(matches!(
                &mob.events[..],
                [MobEvent::Sound { sound: "minecraft:entity.horse.gallop", .. },
                 MobEvent::Sound { sound: "minecraft:entity.baby_horse.breathe", volume, pitch, .. }]
                    if *volume == sound_type.volume * 0.6 && *pitch == sound_type.pitch
            ));

            mob.events.clear();
            mob.silent = true;
            me.random = crate::entity::JavaRandom::new(0);
            play_gallop_sound(me, mob, sound_type);
            assert_eq!(me.random, expected_rng);
            assert!(mob.events.is_empty());
        }).expect("equine row");
    }

    /// 上面盖着雪层时用雪的声音类型（`AbstractHorse.playStepSound` 的 `aboveState`）。
    ///
    /// A snow layer above wins over the block below, as in vanilla's `aboveState`.
    #[test]
    fn a_snow_layer_above_swaps_the_sound_type() {
        let pos = BlockPos::new(0, 63, 0);
        let mut level = crate::testing::TestLevel::new();
        level.set(pos, BlockState(blocks::GRASS_BLOCK));
        level.set(pos.above(), BlockState(blocks::SNOW));
        let mut ticks = crate::BlockTicks::new();
        let world = crate::World::new(&mut level, &mut ticks);
        let mut entity = Entity::by_mob_type(MobType::Horse, Vec3::new(0.5, 64.0, 0.5));
        let heard = entity
            .with_mob(|me, mob| {
                play_step_sound(me, mob, &world, pos, BlockState(blocks::GRASS_BLOCK));
                mob.events.iter().find_map(|event| match event {
                    MobEvent::Sound { volume, pitch, .. } => Some((*volume, *pitch)),
                    _ => None,
                })
            })
            .expect("equine row");
        let snow = BlockState(blocks::SNOW).sound_type();
        assert_eq!(heard, Some((snow.volume * 0.15, snow.pitch)));
    }

    /// 羊驼构造时把导航的 `required_path_length` 设成 40（节点上限 640），`AbstractHorse`
    /// 扬蹄时 `standAnim` 逐刻推进、停下后回落到 0。
    ///
    /// Llamas configure `required_path_length = 40` (640 visited nodes), and
    /// `AbstractHorse.standAnim` ramps up while rearing and decays back to 0.
    #[test]
    fn llama_path_length_and_horse_stand_anim_follow_vanilla() {
        let llama = Mob::new(MobType::Llama);
        assert_eq!(llama.navigation.required_path_length, 40.0);
        assert_eq!(llama.navigation.max_visited_nodes, 640);

        let mut horse = Mob::new(MobType::Horse);
        stand_if_possible(&mut horse);
        tick_end(&mut horse);
        let state = horse.equine.as_ref().expect("horse state");
        assert_eq!(state.stand_anim_o, 0.0);
        assert!((state.stand_anim - 0.45).abs() < 1e-6);
        tick_end(&mut horse);
        let state = horse.equine.as_ref().expect("horse state");
        assert!((state.stand_anim_o - 0.45).abs() < 1e-6);
        assert!(state.stand_anim > state.stand_anim_o);
        for _ in 0..40 {
            tick_end(&mut horse);
        }
        let state = horse.equine.as_ref().expect("horse state");
        assert!(!state.standing);
        assert_eq!(state.stand_anim, 0.0);
    }

    /// `AbstractHorse.onClimbable` 恒假：即使站在梯子里，马科也不会爬。
    ///
    /// `AbstractHorse.onClimbable` is always false, even while standing inside a ladder.
    #[test]
    fn equines_are_never_on_climbable() {
        let mut level = crate::testing::TestLevel::new();
        level.set(BlockPos::new(0, 64, 0), BlockState(blocks::LADDER));
        let mut ticks = crate::BlockTicks::new();
        let world = crate::World::new(&mut level, &mut ticks);
        for kind in [
            MobType::Horse,
            MobType::Donkey,
            MobType::Mule,
            MobType::Llama,
        ] {
            let mut entity = Entity::by_mob_type(kind, Vec3::new(0.5, 64.0, 0.5));
            let climbable = entity
                .with_mob(|me, mob| living::on_climbable(me, mob, &world))
                .expect("equine row");
            assert!(!climbable, "{kind:?} should not climb ladders");
        }
    }
}
