//! 马科共有的属性、储存状态与繁殖继承。
//!
//! Shared equine attributes, persistent state and breeding inheritance.

use mc_bamboo_data::{ItemTag, entity_types, items, registries::attribute, tags::item_tags};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    JavaRandom, Mob, MobType,
    attributes::Attributes,
    interact::{MobInteractRequest, MobInteraction, age_up},
    living,
    nbt::{uuid_field, uuid_tag},
};
use crate::{
    InteractionResult,
    archive::{block_entity, tag},
    entity::Entity,
    random::LegacyRandom,
    state::item_is_tagged,
};

mod donkey;
mod horse;
mod llama;
mod mule;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EquineState {
    pub tame: bool,
    pub temper: i32,
    pub owner: Option<[u8; 16]>,
    pub eating_haystack: bool,
    pub bred: bool,
    pub variant: i32,
    pub strength: i32,
    pub chested: bool,
    pub chest_items: Vec<ItemStack>,
}

pub(super) const fn dimensions(kind: MobType, baby: bool) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[kind.entity_type() as usize];
    let eye = match kind {
        MobType::Horse => entity_types::HORSE_EYE_HEIGHT,
        MobType::Donkey => entity_types::DONKEY_EYE_HEIGHT,
        MobType::Mule => entity_types::MULE_EYE_HEIGHT,
        MobType::Llama => entity_types::LLAMA_EYE_HEIGHT,
        _ => unreachable!(),
    };
    let scale = if !baby {
        1.0
    } else if matches!(kind, MobType::Horse) {
        0.7
    } else {
        0.5
    };
    (facts.width * scale, facts.height * scale, eye * scale)
}

pub(super) fn attributes(kind: MobType) -> Attributes {
    let mut attrs = Attributes::cow();
    let (speed, jump) = if kind == MobType::Horse {
        (f64::from(0.225_f32), 0.7)
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

pub(super) fn finalize_spawn(mob: &mut Mob, random: &LegacyRandom) {
    let state = mob.equine.as_mut().expect("equine spawn state");
    match mob.mob_type {
        MobType::Horse => horse::finalize_spawn(state, random),
        MobType::Llama => llama::finalize_spawn(state, random),
        _ => {}
    }
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

pub(super) fn tempt_tag(kind: MobType) -> ItemTag {
    match kind {
        MobType::Llama => item_tags::LLAMA_TEMPT_ITEMS,
        _ if kind.is_equine() => item_tags::HORSE_TEMPT_ITEMS,
        _ => item_tags::COW_FOOD,
    }
}

pub(super) fn offspring_kind(a: MobType, b: MobType) -> Option<MobType> {
    match (a, b) {
        (MobType::Horse, MobType::Horse) => Some(MobType::Horse),
        (MobType::Donkey, MobType::Donkey) => Some(donkey::PUREBRED),
        (MobType::Horse, MobType::Donkey) | (MobType::Donkey, MobType::Horse) => {
            Some(mule::CROSSBRED)
        }
        (MobType::Llama, MobType::Llama) => Some(MobType::Llama),
        _ => None,
    }
}

pub(super) fn can_parent(mob: &Mob) -> bool {
    mob.equine.as_ref().is_some_and(|state| state.tame)
        && !mob.is_baby()
        && mob.health >= mob.attributes.value(attribute::MAX_HEALTH) as f32
        && mob.in_love > 0
}

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

pub(super) fn save(mob: &Mob, out: &mut Vec<(String, Tag)>) -> tag::Result<()> {
    let Some(state) = &mob.equine else {
        return Ok(());
    };
    use tag::put;
    put(
        out,
        "EatingHaystack",
        Tag::Byte(i8::from(state.eating_haystack)),
    );
    put(out, "Bred", Tag::Byte(i8::from(state.bred)));
    put(out, "Temper", Tag::Int(state.temper));
    put(out, "Tame", Tag::Byte(i8::from(state.tame)));
    if let Some(owner) = &state.owner {
        put(out, "Owner", uuid_tag(owner));
    }
    if mob.mob_type != MobType::Horse {
        put(out, "ChestedHorse", Tag::Byte(i8::from(state.chested)));
        if state.chested && state.chest_items.iter().any(|stack| !stack.is_empty()) {
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

pub(super) fn load(mob: &mut Mob, input: &Tag) {
    let Some(state) = &mut mob.equine else { return };
    state.eating_haystack = tag::bool_or(input, "EatingHaystack", false);
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
        state.variant = if (0..4).contains(&state.variant) {
            state.variant
        } else {
            0
        };
    }
}

fn chest_size(kind: MobType, strength: i32) -> usize {
    3 * if kind == MobType::Llama {
        strength.clamp(1, 5) as usize
    } else {
        5
    }
}

pub(super) fn interact(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    if req.held.is_empty() || !living::is_alive(me, mob) {
        return MobInteraction::pass();
    }
    let llama = mob.mob_type == MobType::Llama;
    let food_tag = if llama {
        item_tags::LLAMA_FOOD
    } else {
        item_tags::HORSE_FOOD
    };
    let (result, used) = if item_is_tagged(req.held.item, food_tag) {
        (InteractionResult::SuccessServer, feed(me, mob, req))
    } else if mob.equine.as_ref().is_some_and(|state| state.tame) {
        (InteractionResult::Success, equip(me, mob, &req.held))
    } else {
        return MobInteraction::pass();
    };
    if used {
        MobInteraction {
            result,
            shrink_held: !req.creative,
            replacement: None,
        }
    } else {
        MobInteraction::pass()
    }
}

fn feed(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> bool {
    let (heal, growth, temper, breed_food) = if mob.mob_type == MobType::Llama {
        match req.held.item {
            items::WHEAT => (2.0, 10, 3, false),
            items::HAY_BLOCK => (10.0, 90, 6, true),
            _ => return false,
        }
    } else {
        match req.held.item {
            items::WHEAT => (2.0, 20, 3, false),
            items::SUGAR => (1.0, 30, 3, false),
            items::HAY_BLOCK => (20.0, 180, 0, false),
            items::APPLE | items::CARROT => (3.0, 60, 3, false),
            items::RED_MUSHROOM => (3.0, 0, 3, false),
            items::GOLDEN_CARROT => (4.0, 60, 5, true),
            items::GOLDEN_APPLE | items::ENCHANTED_GOLDEN_APPLE => (10.0, 240, 10, true),
            _ => return false,
        }
    };
    let tame = mob.equine.as_ref().is_some_and(|state| state.tame);
    let mut used = breed_food && tame && mob.age == 0 && mob.in_love <= 0;
    if used {
        mob.in_love = 600;
        mob.love_cause = Some(req.player_uuid);
        mob.events.push(super::MobEvent::Hearts);
    }
    let max_health = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    if mob.health < max_health && heal > 0.0 {
        mob.health = (mob.health + heal).min(max_health);
        used = true;
    }
    if mob.is_baby() && growth > 0 && !mob.age_locked {
        // 原版服务端也会取三次粒子坐标，即使不发送这个粒子。
        // The server draws all three particle coordinates even though it does not send this particle.
        for _ in 0..3 {
            me.random.next_double();
        }
        age_up(me, mob, growth, false);
        used = true;
    }
    if let Some(state) = &mut mob.equine
        && temper > 0
        && (used || !tame)
        && state.temper < 100
    {
        state.temper = (state.temper.saturating_add(temper)).clamp(0, 100);
        used = true;
    }
    if used && !mob.silent {
        let sound = match mob.mob_type {
            MobType::Horse if mob.is_baby() => "minecraft:entity.horse.eat_baby",
            MobType::Horse => "minecraft:entity.horse.eat",
            MobType::Donkey => "minecraft:entity.donkey.eat",
            MobType::Mule => "minecraft:entity.mule.eat",
            MobType::Llama => "minecraft:entity.llama.eat",
            _ => unreachable!(),
        };
        let pitch = 1.0 + (me.random.next_float() - me.random.next_float()) * 0.2;
        living::play_sound(me, mob, sound, 1.0, pitch);
    }
    used
}

fn equip(me: &mut Entity, mob: &mut Mob, stack: &ItemStack) -> bool {
    let Some(state) = &mut mob.equine else {
        return false;
    };
    let kind = mob.mob_type;
    if stack.item == items::CHEST && kind != MobType::Horse && !state.chested {
        state.chested = true;
        state.chest_items = vec![ItemStack::empty(); chest_size(kind, state.strength)];
        let sound = match kind {
            MobType::Donkey => donkey::CHEST_SOUND,
            MobType::Mule => mule::CHEST_SOUND,
            _ => "minecraft:entity.llama.chest",
        };
        let pitch = (me.random.next_float() - me.random.next_float()) * 0.2 + 1.0;
        living::play_sound(me, mob, sound, 1.0, pitch);
        return true;
    }
    if mob.is_baby() {
        return false;
    }
    let slot = match (kind, stack.item) {
        (MobType::Horse | MobType::Donkey | MobType::Mule, items::SADDLE) => EquipmentSlot::Saddle,
        (
            MobType::Horse,
            items::LEATHER_HORSE_ARMOR
            | items::COPPER_HORSE_ARMOR
            | items::IRON_HORSE_ARMOR
            | items::GOLDEN_HORSE_ARMOR
            | items::DIAMOND_HORSE_ARMOR
            | items::NETHERITE_HORSE_ARMOR,
        ) => EquipmentSlot::Body,
        _ => return false,
    };
    if !mob.equipment.get(slot).is_empty() {
        return false;
    }
    let mut equip = stack.clone();
    equip.count = 1;
    mob.equipment.set(slot, equip);
    mob.equipment
        .set_drop_chance(slot, super::equipment::GUARANTEED_DROP_CHANCE);
    true
}
