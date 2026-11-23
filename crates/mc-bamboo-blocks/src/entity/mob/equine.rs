//! 马、驴、骡、羊驼（官方 `AbstractHorse` 一族）：属性、生成随机化、繁殖继承和存档。
//! 每刻的生活在 [`life`]，目标表在 [`goals`]，右键交互在 [`interact`]。
//!
//! Horses, donkeys, mules and llamas (the vanilla `AbstractHorse` family):
//! attributes, spawn randomization, breeding inheritance and save data. Per-tick
//! life is in [`life`], goal tables in [`goals`], right-click in [`interact`].

use mc_bamboo_data::{ItemTag, entity_types, registries::attribute, tags::item_tags};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    JavaRandom, Mob, MobType,
    attributes::Attributes,
    nbt::{uuid_field, uuid_tag},
};
use crate::{
    archive::{block_entity, tag},
    entity::Entity,
    random::RandomSource,
};

mod death;
mod goals;
pub(super) mod horse;
mod interact;
pub mod inventory;
mod life;
mod llama;
mod ranged;
mod spawn;
pub(super) mod tame;
pub(super) use spawn::finalize_spawn;

pub(super) use death::drop_on_death;
pub(super) use goals::{goals, random_stand_can_use, random_stand_start, targets};
pub(super) use interact::interact;
pub(super) use life::{
    Voice, after_hurt, ai_step_end, ai_step_start, handle_start_jump, is_immobile, land_sound,
    play_step_sound, tick_end, tick_ridden, voice,
};
pub(super) use ranged::{ranged_attack_tick, take_did_spit};

/// 马科同步给客户端的元数据，加扬蹄动画进度（供骑手附着点用）。
///
/// The equine metadata sent to clients, plus the rearing animation progress for
/// the rider attachment point.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EquinePublic {
    pub flags: u8,
    pub variant: i32,
    pub chested: bool,
    pub strength: i32,
    /// `AbstractHorse.standAnimO`：上一刻的扬蹄动画值，计算骑手附着点时用。
    ///
    /// `AbstractHorse.standAnimO`: previous rearing animation value for rider placement.
    pub stand_anim_o: f32,
}

impl EquinePublic {
    /// 从状态里抄出要同步的那几项。
    ///
    /// Copy the synced fields out of the state.
    pub fn of(state: &EquineState) -> Self {
        Self {
            flags: state.flags(),
            variant: state.variant,
            chested: state.chested,
            strength: state.strength,
            stand_anim_o: state.stand_anim_o,
        }
    }

    /// `AbstractHorse.isTamed()`：`DATA_ID_FLAGS` 的第 1 位（2）。
    ///
    /// `AbstractHorse.isTamed()`: bit 1 (value 2) of `DATA_ID_FLAGS`.
    pub const fn is_tamed(self) -> bool {
        self.flags & 2 != 0
    }
}

/// 马科在 `Mob` 之外的状态：`AbstractHorse` 的标志字节（驯服、吃草、扬蹄、张嘴）、
/// 脾气值和主人，箱子马的箱子，马和羊驼的花色，羊驼的力量，以及服务端的几个计时器。
///
/// Equine state beyond `Mob`: the `AbstractHorse` flag byte (tame, eating,
/// standing, open mouth), temper and owner, the chest of chested kinds, the
/// horse and llama variants, llama strength and the server-side timers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EquineState {
    pub tame: bool,
    pub temper: i32,
    pub owner: Option<[u8; 16]>,
    /// `FLAG_EATING`，存档里叫 `EatingHaystack`。
    ///
    /// `FLAG_EATING`, saved as `EatingHaystack`.
    pub eating: bool,
    pub bred: bool,
    /// `FLAG_STANDING`：扬蹄。
    ///
    /// `FLAG_STANDING`: rearing.
    pub standing: bool,
    /// `FLAG_OPEN_MOUTH`。
    pub mouth_open: bool,
    pub eating_counter: i32,
    pub mouth_counter: i32,
    pub stand_counter: i32,
    /// `AbstractHorse.standAnim` 和 `standAnimO`：扬蹄动画进度，决定骑手附着点偏移。不存档。
    ///
    /// `AbstractHorse.standAnim` and `standAnimO`: rearing progress for the rider offset. Not saved.
    pub stand_anim: f32,
    pub stand_anim_o: f32,
    pub tail_counter: i32,
    /// `AbstractHorse.gallopSoundCounter`：被骑着跑时按步数换成奔跑步声。不存档。
    ///
    /// `AbstractHorse.gallopSoundCounter`: turns walked steps into gallop sounds
    /// while ridden. Not saved.
    pub gallop_sound_counter: i32,
    pub variant: i32,
    pub strength: i32,
    pub chested: bool,
    pub chest_items: Vec<ItemStack>,
    /// `Llama.didSpit`：吐完口水置真，回击目标下次判断继续时清掉。不存档。
    ///
    /// `Llama.didSpit`: set after a spit, cleared by the retaliation goal's
    /// next continue check. Not saved.
    pub did_spit: bool,
}

impl EquineState {
    /// `AbstractHorse.DATA_ID_FLAGS` 的字节：2 驯服、8 繁殖过、16 吃草、32 扬蹄、64 张嘴。
    ///
    /// The `DATA_ID_FLAGS` byte: 2 tame, 8 bred, 16 eating, 32 standing, 64 open mouth.
    pub fn flags(&self) -> u8 {
        u8::from(self.tame) << 1
            | u8::from(self.bred) << 3
            | u8::from(self.eating) << 4
            | u8::from(self.standing) << 5
            | u8::from(self.mouth_open) << 6
    }
}

/// 成年和幼年的（宽，高，眼高）：马的幼崽缩到 0.7，驴、骡、羊驼缩到 0.5。
///
/// Adult and baby (width, height, eye height): foals scale by 0.7, other kinds by 0.5.
pub(super) fn dimensions(kind: MobType, baby: bool) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[kind.entity_type() as usize];
    let eye = match kind {
        MobType::Horse => entity_types::HORSE_EYE_HEIGHT,
        MobType::Donkey => entity_types::DONKEY_EYE_HEIGHT,
        MobType::Mule => entity_types::MULE_EYE_HEIGHT,
        MobType::Llama => entity_types::LLAMA_EYE_HEIGHT,
        MobType::SkeletonHorse => entity_types::SKELETON_HORSE_EYE_HEIGHT,
        MobType::ZombieHorse => entity_types::ZOMBIE_HORSE_EYE_HEIGHT,
        _ => unreachable!(),
    };
    let scale = if !baby {
        1.0
    } else if matches!(kind, MobType::Horse | MobType::SkeletonHorse | MobType::ZombieHorse) {
        0.7
    } else {
        0.5
    };
    if baby && matches!(kind, MobType::SkeletonHorse | MobType::ZombieHorse) {
        let (width, height, eye_height) = if kind == MobType::SkeletonHorse {
            (
                entity_types::SKELETON_HORSE_BABY_WIDTH,
                entity_types::SKELETON_HORSE_BABY_HEIGHT,
                entity_types::SKELETON_HORSE_BABY_EYE_HEIGHT,
            )
        } else {
            (
                entity_types::ZOMBIE_HORSE_BABY_WIDTH,
                entity_types::ZOMBIE_HORSE_BABY_HEIGHT,
                entity_types::ZOMBIE_HORSE_BABY_EYE_HEIGHT,
            )
        };
        return (width, height, eye_height);
    }
    (facts.width * scale, facts.height * scale, eye * scale)
}

/// 成年骑手在背上的基础附着点 `(y, z)`，官方 `EntityTypes` 的 `passengerAttachments`：
/// 马 1.44375、驴 1.1125、骡 1.2125、羊驼 (1.37, -0.3)。扬蹄偏移由 `EquinePublic::stand_anim_o`
/// 在游戏层叠加。
///
/// The base attachment point `(y, z)` of an adult rider, from vanilla `EntityTypes`'
/// `passengerAttachments`: horse 1.44375, donkey 1.1125, mule 1.2125, llama
/// (1.37, -0.3). The rearing offset is added from `EquinePublic::stand_anim_o`.
pub fn passenger_attachment(kind: MobType) -> (f32, f32) {
    match kind {
        MobType::Horse => (1.443_75, 0.0),
        MobType::Donkey => (1.112_5, 0.0),
        MobType::Mule => (1.212_5, 0.0),
        MobType::Llama => (1.37, -0.3),
        _ => (0.0, 0.0),
    }
}

/// 剪下装备时的掉落偏移：`EntityAttachments.getAverage(EntityAttachment.PASSENGER)`，
/// 未按朝向旋转。成年取 [`passenger_attachment`]，幼崽按官方 `BABY_DIMENSIONS` 的
/// `PASSENGER` 附着点缩放。
///
/// The drop offset for sheared equipment, `EntityAttachments.getAverage(PASSENGER)`
/// (unrotated): [`passenger_attachment`] for adults, and the scaled `BABY_DIMENSIONS`
/// passenger attachment for babies.
pub(super) fn shear_drop_offset(kind: MobType, baby: bool) -> mc_bamboo_core::Vec3 {
    let (y, z) = if !baby {
        passenger_attachment(kind)
    } else {
        let height = entity_types::FACTS[kind.entity_type() as usize].height;
        match kind {
            MobType::Horse => ((height - 0.125) * 0.7, 0.0),
            MobType::Donkey | MobType::Mule => ((height + 0.031_25) * 0.5, -0.312_5 * 0.5),
            MobType::Llama => ((height - 0.25) * 0.5, -0.3 * 0.5),
            _ => (0.0, 0.0),
        }
    };
    mc_bamboo_core::Vec3::new(0.0, f64::from(y), f64::from(z))
}

/// `createBaseHorseAttributes`，驴骡羊驼再用 `createBaseChestedHorseAttributes` 的速度和跳跃。
///
/// `createBaseHorseAttributes`, with the chested speed and jump for the other kinds.
pub(in crate::entity::mob) fn attributes(kind: MobType) -> Attributes {
    let mut attrs = Attributes::cow();
    let (speed, jump) = if kind == MobType::Horse {
        (f64::from(0.225_f32), 0.7)
    } else if kind == MobType::SkeletonHorse {
        (entity_types::SKELETON_HORSE_MOVEMENT_SPEED, 0.7)
    } else {
        (f64::from(0.175_f32), 0.5)
    };
    for (id, base) in [
        (attribute::MAX_HEALTH, 53.0),
        (attribute::MOVEMENT_SPEED, speed),
        (attribute::JUMP_STRENGTH, jump),
        (attribute::STEP_HEIGHT, 1.0),
        (attribute::SAFE_FALL_DISTANCE, 6.0),
        (attribute::FALL_DAMAGE_MULTIPLIER, 0.5),
    ] {
        attrs.set_base(id, base);
    }
    attrs
}

/// `AbstractHorse.randomizeAttributes`，马额外随机速度和跳跃。
///
/// `AbstractHorse.randomizeAttributes`, with extra speed and jump draws for horses.
fn randomize_attributes(mob: &mut Mob, random: &dyn RandomSource) {
    let max_health = 15.0 + random.next_int(8) as f64 + random.next_int(9) as f64;
    set_base(mob, attribute::MAX_HEALTH, max_health);
    if mob.mob_type == MobType::Horse {
        set_base(
            mob,
            attribute::MOVEMENT_SPEED,
            horse::speed(|| random.next_double()),
        );
        set_base(
            mob,
            attribute::JUMP_STRENGTH,
            horse::jump(|| random.next_double()),
        );
    }
}

fn set_base(mob: &mut Mob, id: i32, base: f64) {
    if let Some(entry) = mob.attributes.instance(id) {
        entry.base = base;
    }
}

/// `TemptGoal` 认的物品标签。
///
/// The item tag `TemptGoal` follows.
pub(super) fn tempt_tag(kind: MobType) -> ItemTag {
    match kind {
        MobType::Llama => item_tags::LLAMA_TEMPT_ITEMS,
        _ if kind.is_equine() => item_tags::HORSE_TEMPT_ITEMS,
        _ => item_tags::COW_FOOD,
    }
}

/// `canMate` + `getBreedOffspring` 的类型：马配驴生骡，骡不育。
///
/// The offspring type: horse with donkey gives a mule, mules are sterile.
pub(super) fn offspring_kind(a: MobType, b: MobType) -> Option<MobType> {
    match (a, b) {
        (MobType::Horse, MobType::Horse) => Some(MobType::Horse),
        (MobType::Donkey, MobType::Donkey) => Some(MobType::Donkey),
        (MobType::Horse, MobType::Donkey) | (MobType::Donkey, MobType::Horse) => {
            Some(MobType::Mule)
        }
        (MobType::Llama, MobType::Llama) => Some(MobType::Llama),
        _ => None,
    }
}

/// `AbstractHorse.canParent`：`!isVehicle() && !isPassenger() && isTamed() && !isBaby() &&
/// 满血 && isInLove()`。Bamboo 的生物不会骑别的生物，`isPassenger()` 恒假。
///
/// `AbstractHorse.canParent`: `!isVehicle() && !isPassenger() && isTamed() && !isBaby()
/// && full health && isInLove()`. Bamboo mobs never ride other mobs, so
/// `isPassenger()` is always false.
pub(super) fn can_parent(mob: &Mob) -> bool {
    !mob.is_vehicle()
        && mob.equine.as_ref().is_some_and(|state| state.tame)
        && !mob.is_baby()
        && mob.health >= mob.attributes.value(attribute::MAX_HEALTH) as f32
        && mob.in_love > 0
}

/// 繁殖出的幼崽：马先抽花色，再按血量、跳跃、速度的顺序遗传，羊驼最后抽力量和毛色。
///
/// The bred baby: horse variants first, then health, jump and speed, llama strength and colour last.
pub fn offspring(
    random: &mut JavaRandom,
    parent: &Mob,
    partner: &Mob,
    position: mc_bamboo_core::Vec3,
) -> Option<Entity> {
    let kind = offspring_kind(parent.mob_type, partner.mob_type)?;
    let mut baby = Entity::by_mob_type(kind, position);
    let first = parent.equine.as_ref()?;
    let second = partner.equine.as_ref()?;
    baby.with_mob(|_, mob| {
        if kind == MobType::Horse {
            mob.equine.as_mut().expect("horse offspring state").variant =
                horse::offspring_variant(first.variant, second.variant, random);
        }
        for (id, min, max) in [
            (attribute::MAX_HEALTH, 15.0, 30.0),
            (
                attribute::JUMP_STRENGTH,
                horse::jump(|| 0.0) as f32,
                horse::jump(|| 1.0) as f32,
            ),
            (
                attribute::MOVEMENT_SPEED,
                horse::speed(|| 0.0) as f32,
                horse::speed(|| 1.0) as f32,
            ),
        ] {
            let inherited = offspring_attribute(
                parent.attributes.base(id),
                partner.attributes.base(id),
                f64::from(min),
                f64::from(max),
                random,
            );
            set_base(mob, id, inherited);
        }
        if kind == MobType::Llama {
            llama::inherit(
                mob.equine.as_mut().expect("llama offspring state"),
                first,
                second,
                random,
            );
        }
    });
    Some(baby)
}

/// `AbstractHorse.createOffspringAttribute`：父母先截断，子代在边界反射。
///
/// Clamp both parents before sampling and reflect the result at the bounds.
pub fn offspring_attribute(a: f64, b: f64, min: f64, max: f64, random: &mut JavaRandom) -> f64 {
    assert!(max > min);
    let a = a.clamp(min, max);
    let b = b.clamp(min, max);
    let margin = 0.15 * (max - min);
    let range = (a - b).abs() + margin * 2.0;
    let average = (a + b) / 2.0;
    let quality = (random.next_double() + random.next_double() + random.next_double()) / 3.0 - 0.5;
    let value = average + range * quality;
    if value > max {
        max - (value - max)
    } else if value < min {
        min + (min - value)
    } else {
        value
    }
}

/// `addAdditionalSaveData` 里马科的键。
///
/// The equine keys of `addAdditionalSaveData`.
pub(super) fn save(mob: &Mob, out: &mut Vec<(String, Tag)>) -> tag::Result<()> {
    let Some(state) = &mob.equine else {
        return Ok(());
    };
    use tag::put;
    put(out, "EatingHaystack", Tag::Byte(i8::from(state.eating)));
    put(out, "Bred", Tag::Byte(i8::from(state.bred)));
    put(out, "Temper", Tag::Int(state.temper));
    // `AbstractHorse.addAdditionalSaveData` 存的是 `isTamed()`（可被覆写的方法），
    // 不是 flag 位；`Camel.isTamed()` 恒真，所以骆驼永远写 `Tame=1`。
    // `AbstractHorse.addAdditionalSaveData` saves `isTamed()` (an overridable
    // method), not the flag bit; `Camel.isTamed()` is always true, so a camel
    // always writes `Tame=1`.
    let tamed = mob.mob_type == MobType::Camel || state.tame;
    put(out, "Tame", Tag::Byte(i8::from(tamed)));
    if let Some(owner) = &state.owner {
        put(out, "Owner", uuid_tag(owner));
    }
    // 马、驴、骡、羊驼在子类 `addAdditionalSaveData` 里补箱子 / 花色 / 力量键；
    // `Camel` 没有覆盖，只有上面的基类键。
    // Horses, donkeys, mules and llamas add chest / variant / strength keys in
    // subclass `addAdditionalSaveData`; `Camel` does not override it, so it keeps
    // only the base keys above.
    if !matches!(mob.mob_type, MobType::Horse | MobType::Camel) {
        put(out, "ChestedHorse", Tag::Byte(i8::from(state.chested)));
        // `output.list("Items", ...)` 一建就放进去，空箱子也写一个空列表。
        // `output.list` inserts the list at once, so an empty chest writes an empty list.
        if state.chested {
            put(out, "Items", block_entity::items_tag(&state.chest_items)?);
        }
    }
    if matches!(mob.mob_type, MobType::Horse | MobType::Llama) {
        put(out, "Variant", Tag::Int(state.variant));
    }
    if mob.mob_type == MobType::Llama {
        put(out, "Strength", Tag::Int(state.strength));
    }
    Ok(())
}

/// `readAdditionalSaveData`：羊驼先读力量，箱子大小按它算。
///
/// `readAdditionalSaveData`: llamas read strength first because the chest size depends on it.
pub(super) fn load(mob: &mut Mob, input: &Tag) {
    let Some(state) = &mut mob.equine else { return };
    state.eating = tag::bool_or(input, "EatingHaystack", false);
    state.bred = tag::bool_or(input, "Bred", false);
    state.temper = tag::int_or(input, "Temper", 0);
    state.tame = tag::bool_or(input, "Tame", false);
    state.owner = uuid_field(input, "Owner");
    if mob.mob_type == MobType::Llama {
        state.strength = tag::int_or(input, "Strength", 0).clamp(1, 5);
    }
    if mob.mob_type != MobType::Horse {
        state.chested = tag::bool_or(input, "ChestedHorse", false);
        if state.chested {
            state.chest_items =
                block_entity::read_items(input, chest_size(mob.mob_type, state.strength))
                    .unwrap_or_default();
        } else {
            state.chest_items.clear();
        }
    }
    if matches!(mob.mob_type, MobType::Horse | MobType::Llama) {
        state.variant = tag::int_or(input, "Variant", 0);
    }
    if mob.mob_type == MobType::Llama {
        // `Llama.Variant.BY_ID` 用 `OutOfBoundsStrategy.CLAMP`。
        // `Llama.Variant.BY_ID` clamps out-of-range ids.
        state.variant = state.variant.clamp(0, 3);
    }
}

/// `AbstractMountInventoryMenu.getInventorySize(getInventoryColumns())`：三行，驴骡五列，羊驼按力量。
///
/// Three rows; five columns for donkeys and mules, `strength` for llamas.
pub(super) fn chest_size(kind: MobType, strength: i32) -> usize {
    3 * if kind == MobType::Llama {
        strength.clamp(1, 5) as usize
    } else {
        5
    }
}
