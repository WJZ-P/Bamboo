//! 存档 `Codec` 的字段读写。
//!
//! 数值范围用 Java `Float.compareTo` 的全序，所以 `-0.0` 不算非负数。
//! `optionalFieldOf` 的默认值按 `Objects.equals` 省略，浮点按位比较。
//! `Either` 编码走会省略信息的那一支（区间端点相同就写成裸数字），解码先试复合标签。
//!
//! Field codecs for the archive format.
//!
//! Numeric ranges use Java's total `Float.compareTo` order, so `-0.0` is not
//! non-negative. Defaults of `optionalFieldOf` are omitted by `Objects.equals`,
//! and floats compare by bits. An `Either` encodes the branch that drops
//! redundant information, and decoding tries the compound first.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::component_registries::consume_effect_type;
use mc_bamboo_nbt::{Tag, parse_snbt_compound};
use uuid::Uuid;

use super::{
    components,
    dynamic::Dyn,
    read::{self, Error, Result},
    stack, text,
    write::{self, Fields},
};
use crate::components::{
    ComponentValue,
    cursor::{Holder, HolderSet, Registry},
    predicate::{
        BlockPredicate, ComponentMatchers, PartialPredicate, PredicateType, PropertyMatcher,
        StateProperties, TypedComponent, ValueMatcher,
    },
    stack::ItemStackTemplate,
    value::{
        AttributeDisplay, AttributeEntry, BannerLayer, BannerPattern, BeeOccupant, ConsumeEffect,
        CustomModelData, DamageReduction, EffectDetails, Equippable, Filterable, FireworkExplosion,
        GameProfile, GlobalPos, Instrument, ItemDamageFunction, KineticCondition,
        MobEffectInstance, PartialProfile, Property, ResolvableFloat, ResolvableInt,
        ResolvableProfile, SignText, SkinPatch, SoundEvent, StewEffect, SwingAnimation, ToolRule,
        TrimMaterial, TrimPattern, TypedEntityData,
    },
};
use crate::text::Component;

const LORE_LIMIT: usize = 256;
const BOOK_PAGES: usize = 100;
const PAGE_CHARS: usize = 1024;
const TITLE_CHARS: usize = 32;
const PLAYER_CHARS: usize = 16;
const PROPERTY_NAME: usize = 64;
const PROPERTY_VALUE: usize = 32767;
const PROPERTY_SIGNATURE: usize = 1024;
const PROPERTY_LIMIT: usize = 16;
const HIDDEN_LIMIT: u32 = 64;
const FIREWORK_LIMIT: usize = 256;
const EQUIP_GENERIC: &str = "minecraft:item.armor.equip_generic";
const GENERIC_EAT: &str = "minecraft:entity.generic.eat";
const SHEARS_SNIP: &str = "minecraft:item.shears.snip";

pub(super) fn int_dyn(value: Result<i32>) -> Result<Dyn> {
    value.map(Dyn::Int)
}

pub(super) fn float_dyn(value: Result<f32>) -> Result<Dyn> {
    value.map(Dyn::Float)
}

pub(super) fn positive_i32(value: i32) -> Result<i32> {
    write::range_i32(value, 1, i32::MAX)
}

pub(super) fn non_negative_i32(value: i32) -> Result<i32> {
    write::range_i32(value, 0, i32::MAX)
}

/// 官方 `ExtraCodecs.floatRange`：`compareTo` 的全序，`-0.0` 小于 `0.0`。
///
/// `ExtraCodecs.floatRange`, using `compareTo`. `-0.0` is less than `0.0`.
pub(super) fn in_f32(value: f32, min: f32, max: f32) -> Result<f32> {
    if value.total_cmp(&min).is_ge() && value.total_cmp(&max).is_le() {
        Ok(value)
    } else {
        Err(Error::new(format!("{value} is not in [{min};{max}]")))
    }
}

pub(super) fn non_negative_f32(value: f32) -> Result<f32> {
    in_f32(value, 0.0, f32::MAX)
}

fn positive_f32(value: f32) -> Result<f32> {
    if value.total_cmp(&0.0).is_gt() && value.total_cmp(&f32::MAX).is_le() {
        Ok(value)
    } else {
        Err(Error::new(format!("{value} is not positive")))
    }
}

pub(super) fn id_name(registry: Registry, id: i32) -> Result<Dyn> {
    write::holder_name(registry, id)
}

pub(super) fn sparse_name(ids: &[i32], labels: &[&str], id: i32) -> Result<Dyn> {
    ids.iter()
        .position(|value| *value == id)
        .and_then(|index| labels.get(index).copied())
        .map(Dyn::string)
        .ok_or_else(|| Error::new(format!("{id} is not a known enum id")))
}

fn sparse_id(ids: &[i32], labels: &[&str], tag: &Tag) -> Result<i32> {
    let name = read::string(tag)?;
    labels
        .iter()
        .position(|entry| *entry == name)
        .and_then(|index| ids.get(index).copied())
        .ok_or_else(|| Error::new(format!("{name} is not a known enum name")))
}

fn empty_list() -> Dyn {
    Dyn::List(Vec::new())
}

fn unit(tag: &Tag) -> Result<()> {
    read::compound(tag).map(|_| ())
}

fn limited(text: &str, max: usize) -> Result<String> {
    if text.encode_utf16().count() > max {
        Err(Error::new(format!("string is longer than {max}")))
    } else {
        Ok(text.to_owned())
    }
}

fn player_name(text: &str) -> Result<String> {
    let name = limited(text, PLAYER_CHARS)?;
    if name
        .chars()
        .any(|character| (character as u32) <= 32 || (character as u32) >= 127)
    {
        return Err(Error::new(format!(
            "player name contained disallowed characters: '{name}'"
        )));
    }
    Ok(name)
}

fn unsigned_byte(tag: &Tag) -> Result<i32> {
    let value = read::java_i32(tag)?;
    if !(-128..=127).contains(&value) {
        return Err(Error::new(format!("{value} is not a byte")));
    }
    Ok(i32::from(value as u8))
}

fn unsigned_byte_dyn(value: i32) -> Result<Dyn> {
    if !(0..=255).contains(&value) {
        return Err(Error::new(format!("{value} is not an unsigned byte")));
    }
    Ok(Dyn::Byte(value as i8))
}

fn ints(tag: &Tag, len: usize) -> Result<Vec<i32>> {
    let values = read::list(tag)?;
    if values.len() != len {
        return Err(Error::new(format!("expected {len} ints")));
    }
    values.iter().map(read::int).collect()
}

fn uuid_dyn(id: Uuid) -> Dyn {
    let (msb, lsb) = id.as_u64_pair();
    Dyn::IntArray(vec![
        (msb >> 32) as i32,
        msb as i32,
        (lsb >> 32) as i32,
        lsb as i32,
    ])
}

fn decode_uuid(tag: &Tag) -> Result<Uuid> {
    let values = ints(tag, 4)?;
    let msb = (u64::from(values[0] as u32) << 32) | u64::from(values[1] as u32);
    let lsb = (u64::from(values[2] as u32) << 32) | u64::from(values[3] as u32);
    Ok(Uuid::from_u64_pair(msb, lsb))
}

fn pos_dyn(pos: BlockPos) -> Dyn {
    Dyn::IntArray(vec![pos.x, pos.y, pos.z])
}

fn decode_pos(tag: &Tag) -> Result<BlockPos> {
    let values = ints(tag, 3)?;
    Ok(BlockPos::new(values[0], values[1], values[2]))
}

fn compound_tag(tag: &Tag) -> Result<Tag> {
    read::compound(tag)?;
    Ok(tag.clone())
}

fn list_tag(tag: &Tag) -> Result<Tag> {
    read::list(tag)?;
    Ok(tag.clone())
}

fn custom_entries(tag: &Tag) -> Result<Vec<(String, Tag)>> {
    if let Tag::String(text) = tag {
        return parse_snbt_compound(text).map_err(|error| Error::new(error.to_string()));
    }
    Ok(read::compound(tag)?.to_vec())
}

pub(super) fn encode_use_effects(can_sprint: bool, interact: bool, speed: f32) -> Result<Dyn> {
    Ok(Fields::new()
        .default("can_sprint", Dyn::Bool(can_sprint), &Dyn::Bool(false))
        .default("interact_vibrations", Dyn::Bool(interact), &Dyn::Bool(true))
        .default(
            "speed_multiplier",
            Dyn::Float(in_f32(speed, 0.0, 1.0)?),
            &Dyn::Float(0.2),
        )
        .build())
}

pub(super) fn encode_lore(lines: &[Component], depth: u32) -> Result<Dyn> {
    if lines.len() > LORE_LIMIT {
        return Err(Error::new("lore has more than 256 lines"));
    }
    Ok(Dyn::List(
        lines
            .iter()
            .map(|line| text::encode(line, depth))
            .collect::<Result<_>>()?,
    ))
}

pub(super) fn encode_enchantments(entries: &[(i32, i32)]) -> Result<Dyn> {
    let mut out = Vec::new();
    for (id, level) in entries {
        let name = read::id_name(Registry::ENCHANTMENT, *id)?.to_owned();
        let level = Dyn::Int(write::range_i32(*level, 1, 255)?);
        if let Some((_, existing)) = out.iter_mut().find(|(key, _)| key == &name) {
            *existing = level;
        } else {
            out.push((name, level));
        }
    }
    Ok(Dyn::Map(out))
}

pub(super) fn encode_adventure(predicates: &[BlockPredicate], depth: u32) -> Result<Dyn> {
    if predicates.is_empty() {
        return Err(Error::new("adventure predicate list is empty"));
    }
    let values = predicates
        .iter()
        .map(|predicate| encode_block(predicate, depth))
        .collect::<Result<Vec<_>>>()?;
    Ok(write::compact(values))
}

pub(super) fn encode_attributes(entries: &[AttributeEntry], depth: u32) -> Result<Dyn> {
    Ok(Dyn::List(
        entries
            .iter()
            .map(|entry| encode_attribute(entry, depth))
            .collect::<Result<_>>()?,
    ))
}

pub(super) fn encode_model(data: &CustomModelData) -> Result<Dyn> {
    let colors = data.colors.iter().copied().map(Dyn::Int).collect();
    Ok(Fields::new()
        .default(
            "floats",
            Dyn::List(data.floats.iter().copied().map(Dyn::Float).collect()),
            &empty_list(),
        )
        .default(
            "flags",
            Dyn::List(data.flags.iter().copied().map(Dyn::Bool).collect()),
            &empty_list(),
        )
        .default(
            "strings",
            Dyn::List(data.strings.iter().cloned().map(Dyn::String).collect()),
            &empty_list(),
        )
        .default("colors", Dyn::List(colors), &empty_list())
        .build())
}

pub(super) fn encode_tooltip(hide: bool, hidden: &[i32]) -> Result<Dyn> {
    let mut names = Vec::new();
    for id in hidden {
        let name = Dyn::string(read::id_name(Registry::DATA_COMPONENT_TYPE, *id)?);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Ok(Fields::new()
        .default("hide_tooltip", Dyn::Bool(hide), &Dyn::Bool(false))
        .default("hidden_components", Dyn::List(names), &empty_list())
        .build())
}

pub(super) fn encode_food(nutrition: i32, saturation: f32, always: bool) -> Result<Dyn> {
    Ok(Fields::new()
        .put("nutrition", Dyn::Int(non_negative_i32(nutrition)?))
        .put("saturation", Dyn::Float(saturation))
        .default("can_always_eat", Dyn::Bool(always), &Dyn::Bool(false))
        .build())
}

pub(super) fn encode_consumable(
    seconds: f32,
    animation: i32,
    sound: &Holder<SoundEvent>,
    particles: bool,
    effects: &[ConsumeEffect],
    depth: u32,
) -> Result<Dyn> {
    let animation = write::enum_name(
        mc_bamboo_data::component_enums::names::ITEM_USE_ANIMATION,
        animation,
    )?;
    let sound = encode_sound(sound)?;
    let effects = effects
        .iter()
        .map(|effect| encode_consume(effect, depth))
        .collect::<Result<Vec<_>>>()?;
    Ok(Fields::new()
        .default(
            "consume_seconds",
            Dyn::Float(non_negative_f32(seconds)?),
            &Dyn::Float(1.6),
        )
        .default("animation", animation, &Dyn::string("eat"))
        .default("sound", sound, &Dyn::string(GENERIC_EAT))
        .default(
            "has_consume_particles",
            Dyn::Bool(particles),
            &Dyn::Bool(true),
        )
        .default("on_consume_effects", Dyn::List(effects), &empty_list())
        .build())
}

pub(super) fn encode_cooldown(seconds: f32, group: &Option<String>) -> Result<Dyn> {
    Ok(Fields::new()
        .put("seconds", Dyn::Float(positive_f32(seconds)?))
        .optional(
            "cooldown_group",
            group.as_ref().map(|name| write::identifier(name)),
        )
        .build())
}

pub(super) fn encode_holder_set_field(
    name: &str,
    registry: Registry,
    set: &HolderSet,
) -> Result<Dyn> {
    Ok(Fields::new()
        .put(name, write::holder_set(registry, set)?)
        .build())
}

pub(super) fn encode_tool(
    rules: &[ToolRule],
    speed: f32,
    damage: i32,
    creative: bool,
) -> Result<Dyn> {
    let rules = rules.iter().map(encode_rule).collect::<Result<Vec<_>>>()?;
    Ok(Fields::new()
        .put("rules", Dyn::List(rules))
        .default("default_mining_speed", Dyn::Float(speed), &Dyn::Float(1.0))
        .default(
            "damage_per_block",
            Dyn::Int(non_negative_i32(damage)?),
            &Dyn::Int(1),
        )
        .default(
            "can_destroy_blocks_in_creative",
            Dyn::Bool(creative),
            &Dyn::Bool(true),
        )
        .build())
}

pub(super) fn encode_weapon(damage: i32, seconds: f32) -> Result<Dyn> {
    Ok(Fields::new()
        .default(
            "item_damage_per_attack",
            Dyn::Int(non_negative_i32(damage)?),
            &Dyn::Int(1),
        )
        .default(
            "disable_blocking_for_seconds",
            Dyn::Float(non_negative_f32(seconds)?),
            &Dyn::Float(0.0),
        )
        .build())
}

pub(super) fn encode_attack_range(
    min_reach: f32,
    max_reach: f32,
    min_creative: f32,
    max_creative: f32,
    margin: f32,
    mob: f32,
) -> Result<Dyn> {
    Ok(Fields::new()
        .default(
            "min_reach",
            Dyn::Float(in_f32(min_reach, 0.0, 64.0)?),
            &Dyn::Float(0.0),
        )
        .default(
            "max_reach",
            Dyn::Float(in_f32(max_reach, 0.0, 64.0)?),
            &Dyn::Float(3.0),
        )
        .default(
            "min_creative_reach",
            Dyn::Float(in_f32(min_creative, 0.0, 64.0)?),
            &Dyn::Float(0.0),
        )
        .default(
            "max_creative_reach",
            Dyn::Float(in_f32(max_creative, 0.0, 64.0)?),
            &Dyn::Float(5.0),
        )
        .default(
            "hitbox_margin",
            Dyn::Float(in_f32(margin, 0.0, 1.0)?),
            &Dyn::Float(0.3),
        )
        .default(
            "mob_factor",
            Dyn::Float(in_f32(mob, 0.0, 2.0)?),
            &Dyn::Float(1.0),
        )
        .build())
}

pub(super) fn encode_positive_field(name: &str, value: i32) -> Result<Dyn> {
    Ok(Fields::new()
        .put(name, Dyn::Int(positive_i32(value)?))
        .build())
}

pub(super) fn encode_equippable(value: &Equippable, _depth: u32) -> Result<Dyn> {
    let slot = write::enum_name(
        mc_bamboo_data::component_enums::names::EQUIPMENT_SLOT,
        value.slot,
    )?;
    let equip = encode_sound(&value.equip_sound)?;
    let shear = encode_sound(&value.shearing_sound)?;
    Ok(Fields::new()
        .put("slot", slot)
        .default("equip_sound", equip, &Dyn::string(EQUIP_GENERIC))
        .optional(
            "asset_id",
            value.asset_id.as_ref().map(|name| write::identifier(name)),
        )
        .optional(
            "camera_overlay",
            value
                .camera_overlay
                .as_ref()
                .map(|name| write::identifier(name)),
        )
        .optional(
            "allowed_entities",
            value
                .allowed_entities
                .as_ref()
                .map(|set| write::holder_set(Registry::ENTITY_TYPE, set))
                .transpose()?,
        )
        .default(
            "dispensable",
            Dyn::Bool(value.dispensable),
            &Dyn::Bool(true),
        )
        .default("swappable", Dyn::Bool(value.swappable), &Dyn::Bool(true))
        .default(
            "damage_on_hurt",
            Dyn::Bool(value.damage_on_hurt),
            &Dyn::Bool(true),
        )
        .default(
            "equip_on_interact",
            Dyn::Bool(value.equip_on_interact),
            &Dyn::Bool(false),
        )
        .default(
            "can_be_sheared",
            Dyn::Bool(value.can_be_sheared),
            &Dyn::Bool(false),
        )
        .default("shearing_sound", shear, &Dyn::string(SHEARS_SNIP))
        .build())
}

pub(super) fn encode_effect_list(name: &str, effects: &[ConsumeEffect], depth: u32) -> Result<Dyn> {
    let effects = effects
        .iter()
        .map(|effect| encode_consume(effect, depth))
        .collect::<Result<Vec<_>>>()?;
    Ok(Fields::new()
        .default(name, Dyn::List(effects), &empty_list())
        .build())
}

pub(super) fn encode_blocks_attacks(
    delay: f32,
    scale: f32,
    reductions: &[DamageReduction],
    item_damage: &ItemDamageFunction,
    bypassed: &Option<HolderSet>,
    block_sound: &Option<Holder<SoundEvent>>,
    disabled_sound: &Option<Holder<SoundEvent>>,
) -> Result<Dyn> {
    let reductions = reductions
        .iter()
        .map(encode_reduction)
        .collect::<Result<Vec<_>>>()?;
    let item_damage = encode_item_damage(item_damage)?;
    Ok(Fields::new()
        .default(
            "block_delay_seconds",
            Dyn::Float(non_negative_f32(delay)?),
            &Dyn::Float(0.0),
        )
        .default(
            "disable_cooldown_scale",
            Dyn::Float(non_negative_f32(scale)?),
            &Dyn::Float(1.0),
        )
        .default(
            "damage_reductions",
            Dyn::List(reductions),
            &default_reductions(),
        )
        .default("item_damage", item_damage, &default_item_damage())
        .optional(
            "bypassed_by",
            bypassed
                .as_ref()
                .map(|set| write::holder_set(Registry::DAMAGE_TYPE, set))
                .transpose()?,
        )
        .optional(
            "block_sound",
            block_sound.as_ref().map(encode_sound).transpose()?,
        )
        .optional(
            "disabled_sound",
            disabled_sound.as_ref().map(encode_sound).transpose()?,
        )
        .build())
}

pub(super) fn encode_piercing(
    knockback: bool,
    dismounts: bool,
    sound: &Option<Holder<SoundEvent>>,
    hit_sound: &Option<Holder<SoundEvent>>,
) -> Result<Dyn> {
    Ok(Fields::new()
        .default("deals_knockback", Dyn::Bool(knockback), &Dyn::Bool(true))
        .default("dismounts", Dyn::Bool(dismounts), &Dyn::Bool(false))
        .optional("sound", sound.as_ref().map(encode_sound).transpose()?)
        .optional(
            "hit_sound",
            hit_sound.as_ref().map(encode_sound).transpose()?,
        )
        .build())
}

pub(super) fn encode_kinetic(
    ticks: (i32, i32),
    conditions: (
        &Option<KineticCondition>,
        &Option<KineticCondition>,
        &Option<KineticCondition>,
    ),
    movement: (f32, f32),
    sounds: (&Option<Holder<SoundEvent>>, &Option<Holder<SoundEvent>>),
) -> Result<Dyn> {
    let (contact, delay) = ticks;
    let (dismount, knockback, damage) = conditions;
    let (forward, multiplier) = movement;
    let (sound, hit_sound) = sounds;
    Ok(Fields::new()
        .default(
            "contact_cooldown_ticks",
            Dyn::Int(non_negative_i32(contact)?),
            &Dyn::Int(10),
        )
        .default(
            "delay_ticks",
            Dyn::Int(non_negative_i32(delay)?),
            &Dyn::Int(0),
        )
        .optional(
            "dismount_conditions",
            dismount.as_ref().map(encode_condition).transpose()?,
        )
        .optional(
            "knockback_conditions",
            knockback.as_ref().map(encode_condition).transpose()?,
        )
        .optional(
            "damage_conditions",
            damage.as_ref().map(encode_condition).transpose()?,
        )
        .default("forward_movement", Dyn::Float(forward), &Dyn::Float(0.0))
        .default(
            "damage_multiplier",
            Dyn::Float(multiplier),
            &Dyn::Float(1.0),
        )
        .optional("sound", sound.as_ref().map(encode_sound).transpose()?)
        .optional(
            "hit_sound",
            hit_sound.as_ref().map(encode_sound).transpose()?,
        )
        .build())
}

pub(super) fn encode_templates(
    items: &[ItemStackTemplate],
    limit: usize,
    depth: u32,
) -> Result<Dyn> {
    if items.len() > limit {
        return Err(Error::new(format!("item list is longer than {limit}")));
    }
    Ok(Dyn::List(
        items
            .iter()
            .map(|item| stack::encode_template(item, depth))
            .collect::<Result<_>>()?,
    ))
}

pub(super) fn encode_potion(
    potion: Option<i32>,
    color: Option<i32>,
    effects: &[MobEffectInstance],
    name: Option<&str>,
    depth: u32,
) -> Result<Dyn> {
    let effects = effects
        .iter()
        .map(|effect| encode_mob_effect(effect, 0))
        .collect::<Result<Vec<_>>>()?;
    let _ = depth;
    Ok(Fields::new()
        .optional(
            "potion",
            potion.map(|id| id_name(Registry::POTION, id)).transpose()?,
        )
        .optional("custom_color", color.map(Dyn::Int))
        .default("custom_effects", Dyn::List(effects), &empty_list())
        .optional("custom_name", name.map(|text| Dyn::String(text.to_owned())))
        .build())
}

pub(super) fn encode_stew(effects: &[StewEffect]) -> Result<Dyn> {
    Ok(Dyn::List(
        effects
            .iter()
            .map(encode_stew_effect)
            .collect::<Result<_>>()?,
    ))
}

pub(super) fn encode_writable(pages: &[Filterable<String>]) -> Result<Dyn> {
    if pages.len() > BOOK_PAGES {
        return Err(Error::new("writable book has more than 100 pages"));
    }
    let pages = pages
        .iter()
        .map(|page| encode_filterable_string(page, PAGE_CHARS))
        .collect::<Result<Vec<_>>>()?;
    Ok(Fields::new()
        .default("pages", Dyn::List(pages), &empty_list())
        .build())
}

pub(super) fn encode_written(
    title: &Filterable<String>,
    author: &str,
    generation: i32,
    pages: &[Filterable<Component>],
    resolved: bool,
    depth: u32,
) -> Result<Dyn> {
    let pages = pages
        .iter()
        .map(|page| encode_filterable_text(page, depth))
        .collect::<Result<Vec<_>>>()?;
    Ok(Fields::new()
        .put("title", encode_filterable_string(title, TITLE_CHARS)?)
        .put("author", Dyn::String(author.to_owned()))
        .default(
            "generation",
            Dyn::Int(write::range_i32(generation, 0, 3)?),
            &Dyn::Int(0),
        )
        .default("pages", Dyn::List(pages), &empty_list())
        .default("resolved", Dyn::Bool(resolved), &Dyn::Bool(false))
        .build())
}

pub(super) fn encode_trim(
    material: &Holder<TrimMaterial>,
    pattern: &Holder<TrimPattern>,
    depth: u32,
) -> Result<Dyn> {
    Ok(Fields::new()
        .put("material", encode_trim_material(material, depth)?)
        .put("pattern", encode_trim_pattern(pattern, depth)?)
        .build())
}

pub(super) fn encode_typed(data: &TypedEntityData, registry: Registry) -> Result<Dyn> {
    let mut entries = vec![(
        "id".to_owned(),
        Dyn::string(read::id_name(registry, data.kind)?),
    )];
    for (key, value) in &data.tag {
        if key != "id" {
            entries.push((key.clone(), Dyn::from_tag(value)));
        }
    }
    Ok(Dyn::Map(entries))
}

pub(super) fn encode_instrument(holder: &Holder<Instrument>, depth: u32) -> Result<Dyn> {
    write::holder(Registry::INSTRUMENT, holder, |instrument| {
        Ok(Fields::new()
            .put("sound_event", encode_sound(&instrument.sound)?)
            .put(
                "use_duration",
                Dyn::Float(non_negative_f32(instrument.use_duration)?),
            )
            .put("range", Dyn::Float(positive_f32(instrument.range)?))
            .default(
                "durability_damage",
                Dyn::Int(non_negative_i32(instrument.durability_damage)?),
                &Dyn::Int(0),
            )
            .put("description", text::encode(&instrument.description, depth)?)
            .build())
    })
}

pub(super) fn encode_trim_material(holder: &Holder<TrimMaterial>, depth: u32) -> Result<Dyn> {
    write::holder(Registry::TRIM_MATERIAL, holder, |material| {
        Ok(Fields::new()
            .put("palette_id", write::identifier(&material.palette_id))
            .put("description", text::encode(&material.description, depth)?)
            .build())
    })
}

pub(super) fn encode_recipes(tag: &Tag) -> Result<Dyn> {
    let items = read::list(tag)?;
    let values = items
        .iter()
        .map(|item| Ok(write::identifier(&read::identifier(item)?)))
        .collect::<Result<Vec<_>>>()?;
    Ok(Dyn::List(values))
}

pub(super) fn encode_lodestone(target: Option<&GlobalPos>, tracked: bool) -> Result<Dyn> {
    let target = target
        .map(|pos| {
            Ok(Fields::new()
                .put("dimension", write::identifier(&pos.dimension))
                .put("pos", pos_dyn(pos.pos))
                .build())
        })
        .transpose()?;
    Ok(Fields::new()
        .optional("target", target)
        .default("tracked", Dyn::Bool(tracked), &Dyn::Bool(true))
        .build())
}

pub(super) fn encode_explosion(explosion: &FireworkExplosion) -> Result<Dyn> {
    let shape = write::enum_name(
        mc_bamboo_data::component_enums::names::FIREWORK_SHAPE,
        explosion.shape,
    )?;
    Ok(Fields::new()
        .put("shape", shape)
        .default(
            "colors",
            Dyn::List(explosion.colors.iter().copied().map(Dyn::Int).collect()),
            &empty_list(),
        )
        .default(
            "fade_colors",
            Dyn::List(
                explosion
                    .fade_colors
                    .iter()
                    .copied()
                    .map(Dyn::Int)
                    .collect(),
            ),
            &empty_list(),
        )
        .default(
            "has_trail",
            Dyn::Bool(explosion.has_trail),
            &Dyn::Bool(false),
        )
        .default(
            "has_twinkle",
            Dyn::Bool(explosion.has_twinkle),
            &Dyn::Bool(false),
        )
        .build())
}

pub(super) fn encode_fireworks(flight: i32, explosions: &[FireworkExplosion]) -> Result<Dyn> {
    if explosions.len() > FIREWORK_LIMIT {
        return Err(Error::new("fireworks have more than 256 explosions"));
    }
    let explosions = explosions
        .iter()
        .map(encode_explosion)
        .collect::<Result<Vec<_>>>()?;
    Ok(Fields::new()
        .default("flight_duration", unsigned_byte_dyn(flight)?, &Dyn::Byte(0))
        .default("explosions", Dyn::List(explosions), &empty_list())
        .build())
}

pub(super) fn encode_profile(profile: &ResolvableProfile, _depth: u32) -> Result<Dyn> {
    let mut fields = Fields::new();
    match &profile.profile {
        Ok(full) => {
            fields = fields
                .put("id", uuid_dyn(full.id))
                .put("name", Dyn::String(player_name(&full.name)?));
            if !full.properties.is_empty() {
                fields = fields.put("properties", encode_properties(&full.properties)?);
            }
        }
        Err(partial) => {
            if let Some(name) = &partial.name {
                fields = fields.put("name", Dyn::String(player_name(name)?));
            }
            if let Some(id) = partial.id {
                fields = fields.put("id", uuid_dyn(id));
            }
            if !partial.properties.is_empty() {
                fields = fields.put("properties", encode_properties(&partial.properties)?);
            }
        }
    }
    Ok(encode_skin(fields, &profile.skin_patch)?.build())
}

pub(super) fn encode_banner(layers: &[BannerLayer], depth: u32) -> Result<Dyn> {
    Ok(Dyn::List(
        layers
            .iter()
            .map(|layer| encode_banner_layer(layer, depth))
            .collect::<Result<_>>()?,
    ))
}

pub(super) fn encode_pot(
    back: &Option<ItemStackTemplate>,
    left: &Option<ItemStackTemplate>,
    right: &Option<ItemStackTemplate>,
    front: &Option<ItemStackTemplate>,
    depth: u32,
) -> Result<Dyn> {
    Ok(Fields::new()
        .optional(
            "back",
            back.as_ref()
                .map(|item| stack::encode_template(item, depth))
                .transpose()?,
        )
        .optional(
            "left",
            left.as_ref()
                .map(|item| stack::encode_template(item, depth))
                .transpose()?,
        )
        .optional(
            "right",
            right
                .as_ref()
                .map(|item| stack::encode_template(item, depth))
                .transpose()?,
        )
        .optional(
            "front",
            front
                .as_ref()
                .map(|item| stack::encode_template(item, depth))
                .transpose()?,
        )
        .build())
}

pub(super) fn encode_container(slots: &[Option<ItemStackTemplate>], depth: u32) -> Result<Dyn> {
    let mut items = Vec::new();
    for (index, slot) in slots.iter().enumerate() {
        let Some(item) = slot else { continue };
        let index = write::range_i32(i32::try_from(index).unwrap_or(i32::MAX), 0, 255)?;
        items.push(
            Fields::new()
                .put("slot", Dyn::Int(index))
                .put("item", stack::encode_template(item, depth)?)
                .build(),
        );
    }
    Ok(Dyn::List(items))
}

pub(super) fn encode_bees(bees: &[BeeOccupant]) -> Result<Dyn> {
    Ok(Dyn::List(
        bees.iter().map(encode_bee).collect::<Result<_>>()?,
    ))
}

pub(super) fn encode_lock(tag: &Tag, depth: u32) -> Result<Dyn> {
    let entries = read::compound(tag)?;
    let mut out = Vec::new();
    for (key, value) in entries {
        let encoded = match key.as_str() {
            "components" => encode_exact_tag(value, depth)?,
            "predicates" => encode_partial_tag(value, depth)?,
            "count" => encode_int_bounds(value)?,
            _ => Dyn::from_tag(value),
        };
        if key == "count" && encoded.same(&Dyn::empty_map()) {
            continue;
        }
        out.push((key.clone(), encoded));
    }
    Ok(Dyn::Map(out))
}

pub(super) fn encode_loot(tag: &Tag) -> Result<Dyn> {
    let entries = read::compound(tag)?;
    let table = read::identifier(read::required(entries, "loot_table")?)?;
    let seed = match read::field(entries, "seed") {
        Some(value) => read::long(value)?,
        None => 0,
    };
    Ok(Fields::new()
        .put("loot_table", write::identifier(&table))
        .default("seed", Dyn::Long(seed), &Dyn::Long(0))
        .build())
}

pub(super) fn encode_sound(holder: &Holder<SoundEvent>) -> Result<Dyn> {
    write::holder(Registry::SOUND_EVENT, holder, |sound| {
        Ok(Fields::new()
            .put("sound_id", write::identifier(&sound.location))
            .optional(
                "range",
                sound
                    .fixed_range
                    .map(positive_f32)
                    .transpose()?
                    .map(Dyn::Float),
            )
            .build())
    })
}

pub(super) fn encode_resolvable_int(value: &ResolvableInt) -> Result<Dyn> {
    match value {
        ResolvableInt::Constant(value) => Ok(Dyn::Int(*value)),
        ResolvableInt::Reference(name) => Ok(write::identifier(name)),
    }
}

fn encode_resolvable_float(value: &ResolvableFloat) -> Result<Dyn> {
    match value {
        ResolvableFloat::Constant(value) => Ok(Dyn::Float(*value)),
        ResolvableFloat::Reference(name) => Ok(write::identifier(name)),
    }
}

pub(super) fn encode_fuel(
    name: &str,
    amount: &ResolvableInt,
    speed: &ResolvableFloat,
) -> Result<Dyn> {
    Ok(Fields::new()
        .put(name, encode_resolvable_int(amount)?)
        .put("speed_multiplier", encode_resolvable_float(speed)?)
        .build())
}

pub(super) fn encode_visibility(types: &HolderSet, visibility: f32) -> Result<Dyn> {
    Ok(Fields::new()
        .put(
            "targeting_entity_types",
            write::holder_set(Registry::ENTITY_TYPE, types)?,
        )
        .put("visibility", Dyn::Float(in_f32(visibility, 0.0, 10.0)?))
        .build())
}

pub(super) fn encode_sign(text: &SignText, depth: u32) -> Result<Dyn> {
    if text.messages.len() != 4 {
        return Err(Error::new("sign text does not have 4 lines"));
    }
    let messages = text
        .messages
        .iter()
        .map(|line| text::encode(line, depth))
        .collect::<Result<Vec<_>>>()?;
    let filtered = match &text.filtered_messages {
        Some(lines) if lines.len() == 4 && lines != &text.messages => Some(Dyn::List(
            lines
                .iter()
                .map(|line| text::encode(line, depth))
                .collect::<Result<_>>()?,
        )),
        Some(lines) if lines.len() != 4 => {
            return Err(Error::new("filtered sign text does not have 4 lines"));
        }
        _ => None,
    };
    let color = write::enum_name(
        mc_bamboo_data::component_enums::names::DYE_COLOR,
        text.color,
    )?;
    Ok(Fields::new()
        .put("messages", Dyn::List(messages))
        .optional("filtered_messages", filtered)
        .always("color", color)
        .always("has_glowing_text", Dyn::Bool(text.has_glowing_text))
        .build())
}

pub(super) fn encode_swing(value: &SwingAnimation) -> Result<Dyn> {
    let kind = write::enum_name(
        mc_bamboo_data::component_enums::names::SWING_ANIMATION_TYPE,
        value.kind,
    )?;
    Ok(Fields::new()
        .default("type", kind, &Dyn::string("whack"))
        .default(
            "duration",
            Dyn::Int(non_negative_i32(value.duration)?),
            &Dyn::Int(6),
        )
        .build())
}

fn encode_rule(rule: &ToolRule) -> Result<Dyn> {
    Ok(Fields::new()
        .put("blocks", write::holder_set(Registry::BLOCK, &rule.blocks)?)
        .optional(
            "speed",
            rule.speed.map(positive_f32).transpose()?.map(Dyn::Float),
        )
        .optional("correct_for_drops", rule.correct_for_drops.map(Dyn::Bool))
        .build())
}

fn encode_reduction(value: &DamageReduction) -> Result<Dyn> {
    Ok(Fields::new()
        .default(
            "horizontal_blocking_angle",
            Dyn::Float(positive_f32(value.horizontal_blocking_angle)?),
            &Dyn::Float(90.0),
        )
        .optional(
            "type",
            value
                .damage_types
                .as_ref()
                .map(|set| write::holder_set(Registry::DAMAGE_TYPE, set))
                .transpose()?,
        )
        .put("base", Dyn::Float(value.base))
        .put("factor", Dyn::Float(value.factor))
        .build())
}

fn default_reductions() -> Dyn {
    Dyn::List(vec![
        Fields::new()
            .put("base", Dyn::Float(0.0))
            .put("factor", Dyn::Float(1.0))
            .build(),
    ])
}

fn encode_item_damage(value: &ItemDamageFunction) -> Result<Dyn> {
    Ok(Fields::new()
        .put("threshold", Dyn::Float(non_negative_f32(value.threshold)?))
        .put("base", Dyn::Float(value.base))
        .put("factor", Dyn::Float(value.factor))
        .build())
}

fn default_item_damage() -> Dyn {
    Fields::new()
        .put("threshold", Dyn::Float(1.0))
        .put("base", Dyn::Float(0.0))
        .put("factor", Dyn::Float(1.0))
        .build()
}

fn encode_condition(value: &KineticCondition) -> Result<Dyn> {
    Ok(Fields::new()
        .put(
            "max_duration_ticks",
            Dyn::Int(non_negative_i32(value.max_duration_ticks)?),
        )
        .default("min_speed", Dyn::Float(value.min_speed), &Dyn::Float(0.0))
        .default(
            "min_relative_speed",
            Dyn::Float(value.min_relative_speed),
            &Dyn::Float(0.0),
        )
        .build())
}

fn encode_attribute(entry: &AttributeEntry, depth: u32) -> Result<Dyn> {
    let display = encode_display(&entry.display, depth)?;
    let default_display = Dyn::Map(vec![("type".to_owned(), Dyn::string("default"))]);
    Ok(Fields::new()
        .put("type", id_name(Registry::ATTRIBUTE, entry.attribute)?)
        .put("id", write::identifier(&entry.modifier.id))
        .put("amount", Dyn::Double(entry.modifier.amount))
        .put(
            "operation",
            write::enum_name(
                mc_bamboo_data::component_enums::names::ATTRIBUTE_OPERATION,
                entry.modifier.operation,
            )?,
        )
        .default(
            "slot",
            write::enum_name(
                mc_bamboo_data::component_enums::names::EQUIPMENT_SLOT_GROUP,
                entry.slot,
            )?,
            &Dyn::string("any"),
        )
        .default("display", display, &default_display)
        .build())
}

fn encode_display(display: &AttributeDisplay, depth: u32) -> Result<Dyn> {
    let (name, value) = match display {
        AttributeDisplay::Default => ("default", None),
        AttributeDisplay::Hidden => ("hidden", None),
        AttributeDisplay::Override(text) => ("override", Some(text::encode(text, depth)?)),
    };
    Ok(Fields::new()
        .put("type", Dyn::string(name))
        .optional("value", value)
        .build())
}

fn encode_consume(effect: &ConsumeEffect, depth: u32) -> Result<Dyn> {
    let (id, body) = match effect {
        ConsumeEffect::ApplyEffects {
            effects,
            probability,
        } => {
            let effects = effects
                .iter()
                .map(|effect| encode_mob_effect(effect, 0))
                .collect::<Result<Vec<_>>>()?;
            (
                consume_effect_type::APPLY_EFFECTS,
                Fields::new().put("effects", Dyn::List(effects)).default(
                    "probability",
                    Dyn::Float(in_f32(*probability, 0.0, 1.0)?),
                    &Dyn::Float(1.0),
                ),
            )
        }
        ConsumeEffect::RemoveEffects { effects } => (
            consume_effect_type::REMOVE_EFFECTS,
            Fields::new().put("effects", write::holder_set(Registry::MOB_EFFECT, effects)?),
        ),
        ConsumeEffect::ClearAllEffects => (consume_effect_type::CLEAR_ALL_EFFECTS, Fields::new()),
        ConsumeEffect::TeleportRandomly {
            diameter,
            directional_particles,
        } => (
            consume_effect_type::TELEPORT_RANDOMLY,
            Fields::new()
                .default(
                    "diameter",
                    Dyn::Float(positive_f32(*diameter)?),
                    &Dyn::Float(16.0),
                )
                .default(
                    "directional_particles",
                    Dyn::Bool(*directional_particles),
                    &Dyn::Bool(true),
                ),
        ),
        ConsumeEffect::PlaySound { sound } => (
            consume_effect_type::PLAY_SOUND,
            Fields::new().put("sound", encode_sound(sound)?),
        ),
    };
    let _ = depth;
    Ok(with_type(
        read::id_name(Registry::CONSUME_EFFECT_TYPE, id)?,
        body,
    ))
}

fn with_type(name: &str, fields: Fields) -> Dyn {
    let Dyn::Map(mut entries) = fields.build() else {
        return Dyn::empty_map();
    };
    entries.insert(0, ("type".to_owned(), Dyn::string(name)));
    Dyn::Map(entries)
}

fn encode_mob_effect(effect: &MobEffectInstance, depth: u32) -> Result<Dyn> {
    let mut fields = Fields::new().put("id", id_name(Registry::MOB_EFFECT, effect.effect)?);
    fields = append_details(fields, &effect.details, depth)?;
    Ok(fields.build())
}

fn append_details(fields: Fields, details: &EffectDetails, depth: u32) -> Result<Fields> {
    if depth >= HIDDEN_LIMIT {
        return Err(Error::new("too many hidden mob effects"));
    }
    let hidden = details
        .hidden_effect
        .as_deref()
        .map(|hidden| {
            let fields = append_details(Fields::new(), hidden, depth + 1)?;
            Ok(fields.build())
        })
        .transpose()?;
    Ok(fields
        .default(
            "amplifier",
            unsigned_byte_dyn(details.amplifier)?,
            &Dyn::Byte(0),
        )
        .default("duration", Dyn::Int(details.duration), &Dyn::Int(0))
        .default("ambient", Dyn::Bool(details.ambient), &Dyn::Bool(false))
        .default(
            "show_particles",
            Dyn::Bool(details.show_particles),
            &Dyn::Bool(true),
        )
        .put("show_icon", Dyn::Bool(details.show_icon))
        .optional("hidden_effect", hidden))
}

fn encode_stew_effect(effect: &StewEffect) -> Result<Dyn> {
    Ok(Fields::new()
        .put("id", id_name(Registry::MOB_EFFECT, effect.effect)?)
        .default("duration", Dyn::Int(effect.duration), &Dyn::Int(160))
        .build())
}

fn encode_filterable_string(value: &Filterable<String>, max: usize) -> Result<Dyn> {
    Ok(Fields::new()
        .put("raw", Dyn::String(limited(&value.raw, max)?))
        .optional(
            "filtered",
            value
                .filtered
                .as_ref()
                .map(|text| limited(text, max).map(Dyn::String))
                .transpose()?,
        )
        .build())
}

fn encode_filterable_text(value: &Filterable<Component>, depth: u32) -> Result<Dyn> {
    Ok(Fields::new()
        .put("raw", text::encode(&value.raw, depth)?)
        .optional(
            "filtered",
            value
                .filtered
                .as_ref()
                .map(|text| text::encode(text, depth))
                .transpose()?,
        )
        .build())
}

fn encode_trim_pattern(holder: &Holder<TrimPattern>, depth: u32) -> Result<Dyn> {
    write::holder(Registry::TRIM_PATTERN, holder, |pattern| {
        Ok(Fields::new()
            .put("asset_id", write::identifier(&pattern.asset_id))
            .put("description", text::encode(&pattern.description, depth)?)
            .always("decal", Dyn::Bool(pattern.decal))
            .build())
    })
}

fn encode_banner_layer(layer: &BannerLayer, depth: u32) -> Result<Dyn> {
    Ok(Fields::new()
        .put("pattern", encode_banner_pattern(&layer.pattern, depth)?)
        .put(
            "color",
            write::enum_name(
                mc_bamboo_data::component_enums::names::DYE_COLOR,
                layer.color,
            )?,
        )
        .build())
}

fn encode_banner_pattern(holder: &Holder<BannerPattern>, _depth: u32) -> Result<Dyn> {
    write::holder(Registry::BANNER_PATTERN, holder, |pattern| {
        Ok(Fields::new()
            .put("asset_id", write::identifier(&pattern.asset_id))
            .put(
                "translation_key",
                Dyn::String(pattern.translation_key.clone()),
            )
            .build())
    })
}

fn encode_bee(bee: &BeeOccupant) -> Result<Dyn> {
    Ok(Fields::new()
        .put(
            "entity_data",
            encode_typed(&bee.entity_data, Registry::ENTITY_TYPE)?,
        )
        .put("ticks_in_hive", Dyn::Int(bee.ticks_in_hive))
        .put("min_ticks_in_hive", Dyn::Int(bee.min_ticks_in_hive))
        .build())
}

fn encode_properties(properties: &[Property]) -> Result<Dyn> {
    if properties.len() > PROPERTY_LIMIT {
        return Err(Error::new("profile has more than 16 properties"));
    }
    Ok(Dyn::List(
        properties
            .iter()
            .map(encode_property)
            .collect::<Result<_>>()?,
    ))
}

fn encode_property(property: &Property) -> Result<Dyn> {
    Ok(Fields::new()
        .put("name", Dyn::String(limited(&property.name, PROPERTY_NAME)?))
        .put(
            "value",
            Dyn::String(limited(&property.value, PROPERTY_VALUE)?),
        )
        .optional(
            "signature",
            property
                .signature
                .as_ref()
                .map(|text| limited(text, PROPERTY_SIGNATURE).map(Dyn::String))
                .transpose()?,
        )
        .build())
}

fn encode_skin(fields: Fields, skin: &SkinPatch) -> Result<Fields> {
    let model = skin.slim.map(|slim| if slim { "slim" } else { "wide" });
    Ok(fields
        .optional(
            "texture",
            skin.body.as_ref().map(|name| write::identifier(name)),
        )
        .optional(
            "cape",
            skin.cape.as_ref().map(|name| write::identifier(name)),
        )
        .optional(
            "elytra",
            skin.elytra.as_ref().map(|name| write::identifier(name)),
        )
        .optional("model", model.map(Dyn::string)))
}

/// 端点相同的整数区间写成裸数字，两端都空写成 `{}`（也就是 ANY）。
///
/// A point range encodes as a bare number. Both ends empty is `{}`, which is ANY.
fn encode_int_bounds(tag: &Tag) -> Result<Dyn> {
    if let Tag::Compound(entries) = tag {
        let min = optional_i32(entries, "min")?;
        let max = optional_i32(entries, "max")?;
        if min.is_some() && min == max {
            return Ok(Dyn::Int(min.unwrap_or_default()));
        }
        return Ok(Fields::new()
            .optional("min", min.map(Dyn::Int))
            .optional("max", max.map(Dyn::Int))
            .build());
    }
    Ok(Dyn::Int(read::int(tag)?))
}

fn optional_i32(entries: &[(String, Tag)], name: &str) -> Result<Option<i32>> {
    read::field(entries, name).map(read::int).transpose()
}

fn encode_exact_tag(tag: &Tag, depth: u32) -> Result<Dyn> {
    let entries = read::compound(tag)?;
    let mut out = Vec::new();
    for (key, value) in entries {
        let name = crate::components::cursor::parse_identifier(key)
            .map_err(|_| Error::new(format!("{key} is not a resource location")))?;
        let kind = read::name_id(Registry::DATA_COMPONENT_TYPE, &name)
            .ok_or_else(|| Error::new(format!("no component {name}")))?;
        if stack::is_transient(kind) {
            return Err(Error::new(format!("{name} is not a persistent component")));
        }
        let decoded = components::decode_value(kind, value, depth)?;
        out.push((name, components::encode_value(&decoded, depth)?));
    }
    Ok(Dyn::Map(out))
}

fn encode_partial_tag(tag: &Tag, depth: u32) -> Result<Dyn> {
    let entries = read::compound(tag)?;
    let mut out = Vec::new();
    for (key, value) in entries {
        let name = crate::components::cursor::parse_identifier(key)
            .map_err(|_| Error::new(format!("{key} is not a resource location")))?;
        if read::name_id(Registry::DATA_COMPONENT_PREDICATE_TYPE, &name).is_some() {
            out.push((name, predicate_dyn(value, depth)?));
        } else if read::name_id(Registry::DATA_COMPONENT_TYPE, &name).is_some() {
            read::compound(value)?;
            out.push((name, Dyn::empty_map()));
        } else {
            return Err(Error::new(format!("unknown predicate {name}")));
        }
    }
    Ok(Dyn::Map(out))
}

/// 谓词标签里已知的布尔字段要抬成 `HashOps` 的布尔，嵌套的精确组件要重走组件 `Codec`。
///
/// Known predicate booleans become `HashOps` booleans, and nested exact components
/// are encoded with the component codec.
fn predicate_dyn(tag: &Tag, depth: u32) -> Result<Dyn> {
    crate::stack_hop::guard(|| predicate_dyn_frame(tag, depth))
}

fn predicate_dyn_frame(tag: &Tag, depth: u32) -> Result<Dyn> {
    match tag {
        Tag::Compound(entries) => {
            let mut out = Vec::new();
            for (key, value) in entries {
                let encoded = match key.as_str() {
                    "components" => encode_exact_tag(value, depth)?,
                    "predicates" => encode_partial_tag(value, depth)?,
                    "resolved" | "has_twinkle" | "has_trail" => Dyn::Bool(read::bool_value(value)?),
                    "nbt" => encode_nbt_predicate(value)?,
                    _ => predicate_dyn(value, depth)?,
                };
                out.push((key.clone(), encoded));
            }
            Ok(Dyn::Map(out))
        }
        Tag::List(items) => Ok(Dyn::List(
            items
                .iter()
                .map(|item| predicate_dyn(item, depth))
                .collect::<Result<_>>()?,
        )),
        other => Ok(Dyn::from_tag(other)),
    }
}

fn encode_nbt_predicate(tag: &Tag) -> Result<Dyn> {
    let compound = if let Tag::String(text) = tag {
        parse_snbt_compound(text).map_err(|error| Error::new(error.to_string()))?
    } else {
        read::compound(tag)?.to_vec()
    };
    Ok(Dyn::String(Tag::Compound(compound).to_snbt()))
}

fn encode_block(predicate: &BlockPredicate, depth: u32) -> Result<Dyn> {
    let mut fields = Fields::new().optional(
        "blocks",
        predicate
            .blocks
            .as_ref()
            .map(|set| write::holder_set(Registry::BLOCK, set))
            .transpose()?,
    );
    if let Some(properties) = &predicate.properties {
        fields = fields.put("state", encode_state(properties)?);
    }
    if let Some(nbt) = &predicate.nbt {
        fields = fields.put("nbt", Dyn::String(Tag::Compound(nbt.clone()).to_snbt()));
    }
    Ok(append_matchers(fields, &predicate.components, depth)?.build())
}

fn encode_state(properties: &StateProperties) -> Result<Dyn> {
    let mut entries = Vec::new();
    for matcher in &properties.0 {
        let value = match &matcher.matcher {
            ValueMatcher::Exact(value) => Dyn::String(value.clone()),
            ValueMatcher::Ranged { min, max } => Fields::new()
                .optional("min", min.clone().map(Dyn::String))
                .optional("max", max.clone().map(Dyn::String))
                .build(),
        };
        entries.push((matcher.name.clone(), value));
    }
    Ok(Dyn::Map(entries))
}

fn append_matchers(fields: Fields, matchers: &ComponentMatchers, depth: u32) -> Result<Fields> {
    let exact = if matchers.exact.is_empty() {
        None
    } else {
        Some(encode_exact(&matchers.exact, depth)?)
    };
    let partial = if matchers.partial.is_empty() {
        None
    } else {
        Some(encode_partial(&matchers.partial, depth)?)
    };
    Ok(fields
        .optional("components", exact)
        .optional("predicates", partial))
}

fn encode_exact(components: &[TypedComponent], depth: u32) -> Result<Dyn> {
    let mut entries = Vec::new();
    for component in components {
        if stack::is_transient(component.kind) {
            continue;
        }
        let name = read::id_name(Registry::DATA_COMPONENT_TYPE, component.kind)?.to_owned();
        let value = components::encode_value(&component.value, depth)?;
        if let Some((_, existing)) = entries.iter_mut().find(|(key, _)| key == &name) {
            *existing = value;
        } else {
            entries.push((name, value));
        }
    }
    Ok(Dyn::Map(entries))
}

fn encode_partial(partial: &[PartialPredicate], depth: u32) -> Result<Dyn> {
    let mut entries = Vec::new();
    for predicate in partial {
        let (registry, id, value) = match predicate.kind {
            PredicateType::Concrete(id) => (
                Registry::DATA_COMPONENT_PREDICATE_TYPE,
                id,
                predicate_dyn(&predicate.value, depth)?,
            ),
            PredicateType::AnyValue(id) => (Registry::DATA_COMPONENT_TYPE, id, Dyn::empty_map()),
        };
        entries.push((read::id_name(registry, id)?.to_owned(), value));
    }
    Ok(Dyn::Map(entries))
}

pub(super) fn decode_early(kind: i32, tag: &Tag, depth: u32) -> Result<Option<ComponentValue>> {
    use mc_bamboo_data::registries::data_component_type as kinds;
    let value = match kind {
        kinds::CUSTOM_DATA => ComponentValue::CustomData(custom_entries(tag)?),
        kinds::BUCKET_ENTITY_DATA => ComponentValue::BucketEntityData(custom_entries(tag)?),
        kinds::MAX_STACK_SIZE => {
            ComponentValue::MaxStackSize(write::range_i32(read::int(tag)?, 1, 99)?)
        }
        kinds::MAX_DAMAGE => ComponentValue::MaxDamage(positive_i32(read::int(tag)?)?),
        kinds::DAMAGE => ComponentValue::Damage(non_negative_i32(read::int(tag)?)?),
        kinds::REPAIR_COST => ComponentValue::RepairCost(non_negative_i32(read::int(tag)?)?),
        kinds::UNBREAKABLE => {
            unit(tag)?;
            ComponentValue::Unbreakable
        }
        kinds::GLIDER => {
            unit(tag)?;
            ComponentValue::Glider
        }
        kinds::INTANGIBLE_PROJECTILE => {
            unit(tag)?;
            ComponentValue::IntangibleProjectile
        }
        kinds::USE_EFFECTS => decode_use_effects(tag)?,
        kinds::CUSTOM_NAME => ComponentValue::CustomName(text::decode(tag)?),
        kinds::ITEM_NAME => ComponentValue::ItemName(text::decode(tag)?),
        kinds::MINIMUM_ATTACK_CHARGE => {
            ComponentValue::MinimumAttackCharge(in_f32(read::float(tag)?, 0.0, 1.0)?)
        }
        kinds::DAMAGE_TYPE => {
            ComponentValue::DamageType(read::registry_id(Registry::DAMAGE_TYPE, tag)?)
        }
        kinds::ITEM_MODEL => ComponentValue::ItemModel(read::identifier(tag)?),
        kinds::TOOLTIP_STYLE => ComponentValue::TooltipStyle(read::identifier(tag)?),
        kinds::LORE => ComponentValue::Lore(decode_lore(tag, depth)?),
        kinds::RARITY => ComponentValue::Rarity(enum_name(names_rarity(), tag)?),
        kinds::ENCHANTMENTS => ComponentValue::Enchantments(decode_enchantments(tag)?),
        kinds::STORED_ENCHANTMENTS => ComponentValue::StoredEnchantments(decode_enchantments(tag)?),
        kinds::CAN_PLACE_ON => ComponentValue::CanPlaceOn(decode_adventure(tag, depth)?),
        kinds::CAN_BREAK => ComponentValue::CanBreak(decode_adventure(tag, depth)?),
        kinds::ATTRIBUTE_MODIFIERS => {
            ComponentValue::AttributeModifiers(decode_attributes(tag, depth)?)
        }
        kinds::CUSTOM_MODEL_DATA => ComponentValue::CustomModelData(decode_model(tag)?),
        kinds::TOOLTIP_DISPLAY => decode_tooltip(tag)?,
        kinds::ENCHANTMENT_GLINT_OVERRIDE => {
            ComponentValue::EnchantmentGlintOverride(read::bool_value(tag)?)
        }
        kinds::FOOD => decode_food(tag)?,
        kinds::CONSUMABLE => decode_consumable(tag, depth)?,
        kinds::USE_REMAINDER => ComponentValue::UseRemainder(stack::decode_template(tag, depth)?),
        kinds::USE_COOLDOWN => decode_cooldown(tag)?,
        kinds::DAMAGE_RESISTANT => ComponentValue::DamageResistant(decode_wrapped_set(
            tag,
            "types",
            Registry::DAMAGE_TYPE,
        )?),
        kinds::TOOL => decode_tool(tag)?,
        kinds::WEAPON => decode_weapon(tag)?,
        kinds::ATTACK_RANGE => decode_attack_range(tag)?,
        kinds::ENCHANTABLE => {
            ComponentValue::Enchantable(positive_i32(required_i32(tag, "value")?)?)
        }
        kinds::EQUIPPABLE => ComponentValue::Equippable(decode_equippable(tag)?),
        kinds::REPAIRABLE => {
            ComponentValue::Repairable(decode_wrapped_set(tag, "items", Registry::ITEM)?)
        }
        kinds::DEATH_PROTECTION => {
            ComponentValue::DeathProtection(decode_effect_list(tag, "death_effects", depth)?)
        }
        kinds::BLOCKS_ATTACKS => decode_blocks_attacks(tag)?,
        kinds::PIERCING_WEAPON => decode_piercing(tag)?,
        kinds::KINETIC_WEAPON => decode_kinetic(tag)?,
        kinds::BLOCK_STATE => ComponentValue::BlockState(decode_string_map(tag)?),
        kinds::DYE => ComponentValue::Dye(dye(tag)?),
        kinds::DYED_COLOR => ComponentValue::DyedColor(decode_rgb(tag)?),
        kinds::ENTITY_DATA => ComponentValue::EntityData(decode_typed(tag, Registry::ENTITY_TYPE)?),
        _ => return Ok(None),
    };
    Ok(Some(value))
}

pub(super) fn decode_items(kind: i32, tag: &Tag, depth: u32) -> Result<Option<ComponentValue>> {
    use mc_bamboo_data::registries::data_component_type as kinds;
    let value = match kind {
        kinds::BLOCK_TRANSFORMER => {
            ComponentValue::BlockTransformer(read::registry_id(Registry::BLOCK_TRANSFORMER, tag)?)
        }
        kinds::VILLAGER_FOOD => {
            ComponentValue::VillagerFood(positive_i32(required_i32(tag, "nutrition")?)?)
        }
        kinds::MAP_ID => ComponentValue::MapId(read::int(tag)?),
        kinds::MAP_DECORATIONS => ComponentValue::MapDecorations(compound_tag(tag)?),
        kinds::DEBUG_STICK_STATE => ComponentValue::DebugStickState(compound_tag(tag)?),
        kinds::CHARGED_PROJECTILES => {
            ComponentValue::ChargedProjectiles(decode_templates(tag, 1024, depth)?)
        }
        kinds::BUNDLE_CONTENTS => {
            ComponentValue::BundleContents(decode_templates(tag, usize::MAX, depth)?)
        }
        kinds::POTION_CONTENTS => decode_potion(tag, depth)?,
        kinds::POTION_DURATION_SCALE => {
            ComponentValue::PotionDurationScale(non_negative_f32(read::float(tag)?)?)
        }
        kinds::SUSPICIOUS_STEW_EFFECTS => ComponentValue::SuspiciousStewEffects(decode_stew(tag)?),
        kinds::WRITABLE_BOOK_CONTENT => ComponentValue::WritableBookContent(decode_writable(tag)?),
        kinds::WRITTEN_BOOK_CONTENT => decode_written(tag, depth)?,
        kinds::TRIM => decode_trim(tag, depth)?,
        kinds::BLOCK_ENTITY_DATA => {
            ComponentValue::BlockEntityData(decode_typed(tag, Registry::BLOCK_ENTITY_TYPE)?)
        }
        kinds::INSTRUMENT => ComponentValue::Instrument(decode_instrument(tag, depth)?),
        kinds::PROVIDES_TRIM_MATERIAL => {
            ComponentValue::ProvidesTrimMaterial(decode_trim_material(tag, depth)?)
        }
        kinds::OMINOUS_BOTTLE_AMPLIFIER => {
            ComponentValue::OminousBottleAmplifier(write::range_i32(read::int(tag)?, 0, 4)?)
        }
        kinds::JUKEBOX_PLAYABLE => {
            ComponentValue::JukeboxPlayable(read::holder_fixed(Registry::JUKEBOX_SONG, tag)?)
        }
        kinds::PROVIDES_BANNER_PATTERNS => {
            ComponentValue::ProvidesBannerPatterns(read::holder_set(Registry::BANNER_PATTERN, tag)?)
        }
        kinds::RECIPES => ComponentValue::Recipes(list_tag(tag)?),
        kinds::LODESTONE_TRACKER => decode_lodestone(tag)?,
        kinds::FIREWORK_EXPLOSION => ComponentValue::FireworkExplosion(decode_explosion(tag)?),
        kinds::FIREWORKS => decode_fireworks(tag)?,
        kinds::PROFILE => ComponentValue::Profile(decode_profile(tag)?),
        kinds::NOTE_BLOCK_SOUND => ComponentValue::NoteBlockSound(read::identifier(tag)?),
        kinds::BANNER_PATTERNS => ComponentValue::BannerPatterns(decode_banner(tag, depth)?),
        kinds::BASE_COLOR => ComponentValue::BaseColor(dye(tag)?),
        kinds::POT_DECORATIONS => decode_pot(tag, depth)?,
        kinds::CONTAINER => ComponentValue::Container(decode_container(tag, depth)?),
        kinds::BEES => ComponentValue::Bees(decode_bees(tag)?),
        kinds::SULFUR_CUBE_CONTENT => {
            ComponentValue::SulfurCubeContent(stack::decode_template(tag, depth)?)
        }
        kinds::LOCK => ComponentValue::Lock(compound_tag(tag)?),
        kinds::CONTAINER_LOOT => ComponentValue::ContainerLoot(compound_tag(tag)?),
        kinds::BREAK_SOUND => ComponentValue::BreakSound(decode_sound(tag)?),
        kinds::COMPOSTABLE => {
            ComponentValue::Compostable(decode_resolvable_int(required(tag, "layers")?)?)
        }
        kinds::COOKING_FUEL => decode_fuel(tag, "burn_time", |amount, speed| {
            ComponentValue::CookingFuel {
                burn_time: amount,
                speed_multiplier: speed,
            }
        })?,
        kinds::BREWING_FUEL => {
            decode_fuel(tag, "uses", |amount, speed| ComponentValue::BrewingFuel {
                uses: amount,
                speed_multiplier: speed,
            })?
        }
        kinds::MOB_VISIBILITY => decode_visibility(tag)?,
        kinds::SIGN_TEXT_FRONT => ComponentValue::SignTextFront(decode_sign(tag, depth)?),
        kinds::SIGN_TEXT_BACK => ComponentValue::SignTextBack(decode_sign(tag, depth)?),
        kinds::WAXED => {
            unit(tag)?;
            ComponentValue::Waxed
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

pub(super) fn decode_late(kind: i32, tag: &Tag, _depth: u32) -> Result<Option<ComponentValue>> {
    use mc_bamboo_data::{component_enums::names, registries::data_component_type as kinds};
    let value = match kind {
        kinds::CREATIVE_SLOT_LOCK | kinds::ADDITIONAL_TRADE_COST | kinds::MAP_POST_PROCESSING => {
            return Err(Error::new("component is not persistent"));
        }
        kinds::ATTACK_ANIMATION => ComponentValue::AttackAnimation(decode_swing(tag)?),
        kinds::INTERACT_ANIMATION => ComponentValue::InteractAnimation(decode_swing(tag)?),
        kinds::VILLAGER_VARIANT => {
            ComponentValue::VillagerVariant(read::registry_id(Registry::VILLAGER_TYPE, tag)?)
        }
        kinds::WOLF_VARIANT => {
            ComponentValue::WolfVariant(read::registry_id(Registry::WOLF_VARIANT, tag)?)
        }
        kinds::WOLF_SOUND_VARIANT => {
            ComponentValue::WolfSoundVariant(read::registry_id(Registry::WOLF_SOUND_VARIANT, tag)?)
        }
        kinds::WOLF_COLLAR => ComponentValue::WolfCollar(dye(tag)?),
        kinds::CAT_COLLAR => ComponentValue::CatCollar(dye(tag)?),
        kinds::SHEEP_COLOR => ComponentValue::SheepColor(dye(tag)?),
        kinds::SHULKER_COLOR => ComponentValue::ShulkerColor(dye(tag)?),
        kinds::CUSHION_COLOR => ComponentValue::CushionColor(dye(tag)?),
        kinds::TROPICAL_FISH_BASE_COLOR => ComponentValue::TropicalFishBaseColor(dye(tag)?),
        kinds::TROPICAL_FISH_PATTERN_COLOR => ComponentValue::TropicalFishPatternColor(dye(tag)?),
        kinds::FOX_VARIANT => ComponentValue::FoxVariant(read::enum_id(names::FOX_VARIANT, tag)?),
        kinds::SALMON_SIZE => ComponentValue::SalmonSize(read::enum_id(names::SALMON_SIZE, tag)?),
        kinds::PARROT_VARIANT => {
            ComponentValue::ParrotVariant(read::enum_id(names::PARROT_VARIANT, tag)?)
        }
        kinds::TROPICAL_FISH_PATTERN => ComponentValue::TropicalFishPattern(sparse_id(
            mc_bamboo_data::component_enums::sparse::TROPICAL_FISH_PATTERN,
            names::TROPICAL_FISH_PATTERN,
            tag,
        )?),
        kinds::MOOSHROOM_VARIANT => {
            ComponentValue::MooshroomVariant(read::enum_id(names::MOOSHROOM_VARIANT, tag)?)
        }
        kinds::RABBIT_VARIANT => ComponentValue::RabbitVariant(sparse_id(
            mc_bamboo_data::component_enums::sparse::RABBIT_VARIANT,
            names::RABBIT_VARIANT,
            tag,
        )?),
        kinds::PIG_VARIANT => {
            ComponentValue::PigVariant(read::registry_id(Registry::PIG_VARIANT, tag)?)
        }
        kinds::PIG_SOUND_VARIANT => {
            ComponentValue::PigSoundVariant(read::registry_id(Registry::PIG_SOUND_VARIANT, tag)?)
        }
        kinds::COW_VARIANT => {
            ComponentValue::CowVariant(read::registry_id(Registry::COW_VARIANT, tag)?)
        }
        kinds::COW_SOUND_VARIANT => {
            ComponentValue::CowSoundVariant(read::registry_id(Registry::COW_SOUND_VARIANT, tag)?)
        }
        kinds::CHICKEN_VARIANT => {
            ComponentValue::ChickenVariant(read::registry_id(Registry::CHICKEN_VARIANT, tag)?)
        }
        kinds::CHICKEN_SOUND_VARIANT => ComponentValue::ChickenSoundVariant(read::registry_id(
            Registry::CHICKEN_SOUND_VARIANT,
            tag,
        )?),
        kinds::ZOMBIE_NAUTILUS_VARIANT => ComponentValue::ZombieNautilusVariant(read::registry_id(
            Registry::ZOMBIE_NAUTILUS_VARIANT,
            tag,
        )?),
        kinds::FROG_VARIANT => {
            ComponentValue::FrogVariant(read::registry_id(Registry::FROG_VARIANT, tag)?)
        }
        kinds::HORSE_VARIANT => {
            ComponentValue::HorseVariant(read::enum_id(names::HORSE_VARIANT, tag)?)
        }
        kinds::PAINTING_VARIANT => {
            ComponentValue::PaintingVariant(read::holder_fixed(Registry::PAINTING_VARIANT, tag)?)
        }
        kinds::LLAMA_VARIANT => {
            ComponentValue::LlamaVariant(read::enum_id(names::LLAMA_VARIANT, tag)?)
        }
        kinds::AXOLOTL_VARIANT => {
            ComponentValue::AxolotlVariant(read::enum_id(names::AXOLOTL_VARIANT, tag)?)
        }
        kinds::CAT_VARIANT => {
            ComponentValue::CatVariant(read::registry_id(Registry::CAT_VARIANT, tag)?)
        }
        kinds::CAT_SOUND_VARIANT => {
            ComponentValue::CatSoundVariant(read::registry_id(Registry::CAT_SOUND_VARIANT, tag)?)
        }
        kinds::PROVIDES_POTTERY_PATTERN => ComponentValue::ProvidesPotteryPattern(
            read::registry_id(Registry::DECORATED_POT_PATTERN, tag)?,
        ),
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn names_rarity() -> &'static [&'static str] {
    mc_bamboo_data::component_enums::names::RARITY
}

fn enum_name(names: &[&str], tag: &Tag) -> Result<i32> {
    read::enum_id(names, tag)
}

fn dye(tag: &Tag) -> Result<i32> {
    read::enum_id(mc_bamboo_data::component_enums::names::DYE_COLOR, tag)
}

fn required<'a>(tag: &'a Tag, name: &str) -> Result<&'a Tag> {
    read::required(read::compound(tag)?, name)
}

fn required_i32(tag: &Tag, name: &str) -> Result<i32> {
    read::int(required(tag, name)?)
}

fn opt_bool(entries: &[(String, Tag)], name: &str, default: bool) -> Result<bool> {
    match read::field(entries, name) {
        Some(tag) => read::bool_value(tag),
        None => Ok(default),
    }
}

fn opt_i32(entries: &[(String, Tag)], name: &str, default: i32, min: i32, max: i32) -> Result<i32> {
    match read::field(entries, name) {
        Some(tag) => write::range_i32(read::int(tag)?, min, max),
        None => Ok(default),
    }
}

fn opt_f32(entries: &[(String, Tag)], name: &str, default: f32, min: f32, max: f32) -> Result<f32> {
    match read::field(entries, name) {
        Some(tag) => in_f32(read::float(tag)?, min, max),
        None => Ok(default),
    }
}

fn decode_use_effects(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::UseEffects {
        can_sprint: opt_bool(entries, "can_sprint", false)?,
        interact_vibrations: opt_bool(entries, "interact_vibrations", true)?,
        speed_multiplier: opt_f32(entries, "speed_multiplier", 0.2, 0.0, 1.0)?,
    })
}

fn decode_lore(tag: &Tag, _depth: u32) -> Result<Vec<Component>> {
    let items = read::list(tag)?;
    if items.len() > LORE_LIMIT {
        return Err(Error::new("lore has more than 256 lines"));
    }
    items.iter().map(text::decode).collect()
}

fn decode_enchantments(tag: &Tag) -> Result<Vec<(i32, i32)>> {
    let mut out = Vec::new();
    for (key, value) in read::compound(tag)? {
        let id = read::registry_id(Registry::ENCHANTMENT, &Tag::String(key.clone()))?;
        let level = write::range_i32(read::int(value)?, 1, 255)?;
        if let Some((_, existing)) = out.iter_mut().find(|(kind, _)| *kind == id) {
            *existing = level;
        } else {
            out.push((id, level));
        }
    }
    Ok(out)
}

fn decode_adventure(tag: &Tag, depth: u32) -> Result<Vec<BlockPredicate>> {
    let items = if matches!(tag, Tag::List(_)) {
        read::list(tag)?
    } else {
        vec![tag.clone()]
    };
    if items.is_empty() {
        return Err(Error::new("adventure predicate list is empty"));
    }
    items.iter().map(|item| decode_block(item, depth)).collect()
}

fn decode_block(tag: &Tag, depth: u32) -> Result<BlockPredicate> {
    let entries = read::compound(tag)?;
    Ok(BlockPredicate {
        blocks: read::field(entries, "blocks")
            .map(|value| read::holder_set(Registry::BLOCK, value))
            .transpose()?,
        properties: read::field(entries, "state")
            .map(decode_state)
            .transpose()?,
        nbt: read::field(entries, "nbt")
            .map(decode_nbt_predicate)
            .transpose()?,
        components: decode_matchers(entries, depth)?,
    })
}

fn decode_state(tag: &Tag) -> Result<StateProperties> {
    let mut matchers = Vec::new();
    for (name, value) in read::map(tag)? {
        let matcher = if let Tag::String(exact) = value {
            ValueMatcher::Exact(exact.clone())
        } else {
            let fields = read::compound(value)?;
            ValueMatcher::Ranged {
                min: read::field(fields, "min")
                    .map(|tag| read::string(tag).map(str::to_owned))
                    .transpose()?,
                max: read::field(fields, "max")
                    .map(|tag| read::string(tag).map(str::to_owned))
                    .transpose()?,
            }
        };
        matchers.push(PropertyMatcher {
            name: name.clone(),
            matcher,
        });
    }
    Ok(StateProperties(matchers))
}

fn decode_nbt_predicate(tag: &Tag) -> Result<Vec<(String, Tag)>> {
    if let Tag::String(text) = tag {
        return parse_snbt_compound(text).map_err(|error| Error::new(error.to_string()));
    }
    Ok(read::compound(tag)?.to_vec())
}

fn decode_matchers(entries: &[(String, Tag)], depth: u32) -> Result<ComponentMatchers> {
    let exact = match read::field(entries, "components") {
        Some(tag) => decode_exact_components(tag, depth)?,
        None => Vec::new(),
    };
    let partial = match read::field(entries, "predicates") {
        Some(tag) => decode_partial_components(tag)?,
        None => Vec::new(),
    };
    Ok(ComponentMatchers { exact, partial })
}

fn decode_exact_components(tag: &Tag, depth: u32) -> Result<Vec<TypedComponent>> {
    let mut out = Vec::new();
    for (key, value) in read::compound(tag)? {
        let name = crate::components::cursor::parse_identifier(key)
            .map_err(|_| Error::new(format!("{key} is not a resource location")))?;
        let kind = read::name_id(Registry::DATA_COMPONENT_TYPE, &name)
            .ok_or_else(|| Error::new(format!("no component {name}")))?;
        if stack::is_transient(kind) {
            return Err(Error::new(format!("{name} is not a persistent component")));
        }
        let component = TypedComponent {
            kind,
            value: components::decode_value(kind, value, depth)?,
        };
        if let Some(existing) = out
            .iter_mut()
            .find(|item: &&mut TypedComponent| item.kind == kind)
        {
            *existing = component;
        } else {
            out.push(component);
        }
    }
    Ok(out)
}

fn decode_partial_components(tag: &Tag) -> Result<Vec<PartialPredicate>> {
    let mut out = Vec::new();
    for (key, value) in read::compound(tag)? {
        let name = crate::components::cursor::parse_identifier(key)
            .map_err(|_| Error::new(format!("{key} is not a resource location")))?;
        if let Some(id) = read::name_id(Registry::DATA_COMPONENT_PREDICATE_TYPE, &name) {
            out.push(PartialPredicate {
                kind: PredicateType::Concrete(id),
                value: value.clone(),
            });
        } else if let Some(id) = read::name_id(Registry::DATA_COMPONENT_TYPE, &name) {
            read::compound(value)?;
            out.push(PartialPredicate {
                kind: PredicateType::AnyValue(id),
                value: Tag::Compound(Vec::new()),
            });
        } else {
            return Err(Error::new(format!("unknown predicate {name}")));
        }
    }
    Ok(out)
}

fn decode_attributes(tag: &Tag, depth: u32) -> Result<Vec<AttributeEntry>> {
    read::list(tag)?
        .iter()
        .map(|item| decode_attribute(item, depth))
        .collect()
}

fn decode_attribute(tag: &Tag, depth: u32) -> Result<AttributeEntry> {
    let entries = read::compound(tag)?;
    let slot = match read::field(entries, "slot") {
        Some(tag) => read::enum_id(
            mc_bamboo_data::component_enums::names::EQUIPMENT_SLOT_GROUP,
            tag,
        )?,
        None => 0,
    };
    let display = match read::field(entries, "display") {
        Some(tag) => decode_display(tag, depth)?,
        None => AttributeDisplay::Default,
    };
    Ok(AttributeEntry {
        attribute: read::registry_id(Registry::ATTRIBUTE, read::required(entries, "type")?)?,
        modifier: crate::components::value::AttributeModifier {
            id: read::identifier(read::required(entries, "id")?)?,
            amount: read::double(read::required(entries, "amount")?)?,
            operation: read::enum_id(
                mc_bamboo_data::component_enums::names::ATTRIBUTE_OPERATION,
                read::required(entries, "operation")?,
            )?,
        },
        slot,
        display,
    })
}

fn decode_display(tag: &Tag, _depth: u32) -> Result<AttributeDisplay> {
    let entries = read::compound(tag)?;
    match read::string(read::required(entries, "type")?)? {
        "default" => Ok(AttributeDisplay::Default),
        "hidden" => Ok(AttributeDisplay::Hidden),
        "override" => Ok(AttributeDisplay::Override(text::decode(read::required(
            entries, "value",
        )?)?)),
        other => Err(Error::new(format!("unknown attribute display {other}"))),
    }
}

fn decode_model(tag: &Tag) -> Result<CustomModelData> {
    let entries = read::compound(tag)?;
    Ok(CustomModelData {
        floats: optional_float_list(entries, "floats")?,
        flags: optional_bool_list(entries, "flags")?,
        strings: optional_string_list(entries, "strings")?,
        colors: match read::field(entries, "colors") {
            Some(tag) => read::list(tag)?
                .iter()
                .map(decode_rgb)
                .collect::<Result<_>>()?,
            None => Vec::new(),
        },
    })
}

fn optional_float_list(entries: &[(String, Tag)], name: &str) -> Result<Vec<f32>> {
    match read::field(entries, name) {
        Some(tag) => read::list(tag)?.iter().map(read::float).collect(),
        None => Ok(Vec::new()),
    }
}

fn optional_bool_list(entries: &[(String, Tag)], name: &str) -> Result<Vec<bool>> {
    match read::field(entries, name) {
        Some(tag) => read::list(tag)?.iter().map(read::bool_value).collect(),
        None => Ok(Vec::new()),
    }
}

fn optional_string_list(entries: &[(String, Tag)], name: &str) -> Result<Vec<String>> {
    match read::field(entries, name) {
        Some(tag) => read::list(tag)?
            .iter()
            .map(|item| read::string(item).map(str::to_owned))
            .collect(),
        None => Ok(Vec::new()),
    }
}

fn decode_rgb(tag: &Tag) -> Result<i32> {
    if let Tag::List(items) = tag {
        if items.len() != 3 {
            return Err(Error::new("rgb color needs 3 channels"));
        }
        return Ok(color_from_float(
            read::float(&items[0])?,
            read::float(&items[1])?,
            read::float(&items[2])?,
        ));
    }
    read::int(tag)
}

fn color_from_float(red: f32, green: f32, blue: f32) -> i32 {
    let channel = |value: f32| (value * 255.0).floor() as i32;
    let pack = |value: i32| value & 0xff;
    (pack(channel(1.0)) << 24)
        | (pack(channel(red)) << 16)
        | (pack(channel(green)) << 8)
        | pack(channel(blue))
}

fn decode_tooltip(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    let mut hidden = Vec::new();
    if let Some(tag) = read::field(entries, "hidden_components") {
        for item in &read::list(tag)? {
            let id = read::registry_id(Registry::DATA_COMPONENT_TYPE, item)?;
            if !hidden.contains(&id) {
                hidden.push(id);
            }
        }
    }
    Ok(ComponentValue::TooltipDisplay {
        hide_tooltip: opt_bool(entries, "hide_tooltip", false)?,
        hidden_components: hidden,
    })
}

fn decode_food(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::Food {
        nutrition: non_negative_i32(read::int(read::required(entries, "nutrition")?)?)?,
        saturation: read::float(read::required(entries, "saturation")?)?,
        can_always_eat: opt_bool(entries, "can_always_eat", false)?,
    })
}

fn decode_consumable(tag: &Tag, depth: u32) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    let animation = match read::field(entries, "animation") {
        Some(tag) => read::enum_id(
            mc_bamboo_data::component_enums::names::ITEM_USE_ANIMATION,
            tag,
        )?,
        None => 1,
    };
    let sound = match read::field(entries, "sound") {
        Some(tag) => decode_sound(tag)?,
        None => sound_id(GENERIC_EAT)?,
    };
    Ok(ComponentValue::Consumable {
        consume_seconds: opt_f32(entries, "consume_seconds", 1.6, 0.0, f32::MAX)?,
        animation,
        sound,
        has_consume_particles: opt_bool(entries, "has_consume_particles", true)?,
        on_consume_effects: match read::field(entries, "on_consume_effects") {
            Some(tag) => read::list(tag)?
                .iter()
                .map(|item| decode_consume(item, depth))
                .collect::<Result<_>>()?,
            None => Vec::new(),
        },
    })
}

fn sound_id(name: &str) -> Result<Holder<SoundEvent>> {
    read::holder_fixed(Registry::SOUND_EVENT, &Tag::String(name.to_owned()))
}

fn decode_cooldown(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::UseCooldown {
        seconds: positive_f32(read::float(read::required(entries, "seconds")?)?)?,
        cooldown_group: read::field(entries, "cooldown_group")
            .map(read::identifier)
            .transpose()?,
    })
}

fn decode_wrapped_set(tag: &Tag, name: &str, registry: Registry) -> Result<HolderSet> {
    read::holder_set(registry, required(tag, name)?)
}

fn decode_tool(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    let rules = read::list(read::required(entries, "rules")?)?
        .iter()
        .map(decode_rule)
        .collect::<Result<Vec<_>>>()?;
    Ok(ComponentValue::Tool {
        rules,
        default_mining_speed: match read::field(entries, "default_mining_speed") {
            Some(tag) => read::float(tag)?,
            None => 1.0,
        },
        damage_per_block: opt_i32(entries, "damage_per_block", 1, 0, i32::MAX)?,
        can_destroy_blocks_in_creative: opt_bool(entries, "can_destroy_blocks_in_creative", true)?,
    })
}

fn decode_rule(tag: &Tag) -> Result<ToolRule> {
    let entries = read::compound(tag)?;
    Ok(ToolRule {
        blocks: read::holder_set(Registry::BLOCK, read::required(entries, "blocks")?)?,
        speed: read::field(entries, "speed")
            .map(|tag| positive_f32(read::float(tag)?))
            .transpose()?,
        correct_for_drops: read::field(entries, "correct_for_drops")
            .map(read::bool_value)
            .transpose()?,
    })
}

fn decode_weapon(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::Weapon {
        item_damage_per_attack: opt_i32(entries, "item_damage_per_attack", 1, 0, i32::MAX)?,
        disable_blocking_for_seconds: opt_f32(
            entries,
            "disable_blocking_for_seconds",
            0.0,
            0.0,
            f32::MAX,
        )?,
    })
}

fn decode_attack_range(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::AttackRange {
        min_reach: opt_f32(entries, "min_reach", 0.0, 0.0, 64.0)?,
        max_reach: opt_f32(entries, "max_reach", 3.0, 0.0, 64.0)?,
        min_creative_reach: opt_f32(entries, "min_creative_reach", 0.0, 0.0, 64.0)?,
        max_creative_reach: opt_f32(entries, "max_creative_reach", 5.0, 0.0, 64.0)?,
        hitbox_margin: opt_f32(entries, "hitbox_margin", 0.3, 0.0, 1.0)?,
        mob_factor: opt_f32(entries, "mob_factor", 1.0, 0.0, 2.0)?,
    })
}

fn decode_equippable(tag: &Tag) -> Result<Equippable> {
    let entries = read::compound(tag)?;
    Ok(Equippable {
        slot: read::enum_id(
            mc_bamboo_data::component_enums::names::EQUIPMENT_SLOT,
            read::required(entries, "slot")?,
        )?,
        equip_sound: match read::field(entries, "equip_sound") {
            Some(tag) => decode_sound(tag)?,
            None => sound_id(EQUIP_GENERIC)?,
        },
        asset_id: read::field(entries, "asset_id")
            .map(read::identifier)
            .transpose()?,
        camera_overlay: read::field(entries, "camera_overlay")
            .map(read::identifier)
            .transpose()?,
        allowed_entities: read::field(entries, "allowed_entities")
            .map(|tag| read::holder_set(Registry::ENTITY_TYPE, tag))
            .transpose()?,
        dispensable: opt_bool(entries, "dispensable", true)?,
        swappable: opt_bool(entries, "swappable", true)?,
        damage_on_hurt: opt_bool(entries, "damage_on_hurt", true)?,
        equip_on_interact: opt_bool(entries, "equip_on_interact", false)?,
        can_be_sheared: opt_bool(entries, "can_be_sheared", false)?,
        shearing_sound: match read::field(entries, "shearing_sound") {
            Some(tag) => decode_sound(tag)?,
            None => sound_id(SHEARS_SNIP)?,
        },
    })
}

fn decode_effect_list(tag: &Tag, name: &str, depth: u32) -> Result<Vec<ConsumeEffect>> {
    let entries = read::compound(tag)?;
    match read::field(entries, name) {
        Some(tag) => read::list(tag)?
            .iter()
            .map(|item| decode_consume(item, depth))
            .collect(),
        None => Ok(Vec::new()),
    }
}

fn decode_blocks_attacks(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    let reductions = match read::field(entries, "damage_reductions") {
        Some(tag) => read::list(tag)?
            .iter()
            .map(decode_reduction)
            .collect::<Result<_>>()?,
        None => vec![DamageReduction {
            horizontal_blocking_angle: 90.0,
            damage_types: None,
            base: 0.0,
            factor: 1.0,
        }],
    };
    let item_damage = match read::field(entries, "item_damage") {
        Some(tag) => decode_item_damage(tag)?,
        None => ItemDamageFunction {
            threshold: 1.0,
            base: 0.0,
            factor: 1.0,
        },
    };
    Ok(ComponentValue::BlocksAttacks {
        block_delay_seconds: opt_f32(entries, "block_delay_seconds", 0.0, 0.0, f32::MAX)?,
        disable_cooldown_scale: opt_f32(entries, "disable_cooldown_scale", 1.0, 0.0, f32::MAX)?,
        damage_reductions: reductions,
        item_damage,
        bypassed_by: read::field(entries, "bypassed_by")
            .map(|tag| read::holder_set(Registry::DAMAGE_TYPE, tag))
            .transpose()?,
        block_sound: read::field(entries, "block_sound")
            .map(decode_sound)
            .transpose()?,
        disabled_sound: read::field(entries, "disabled_sound")
            .map(decode_sound)
            .transpose()?,
    })
}

fn decode_reduction(tag: &Tag) -> Result<DamageReduction> {
    let entries = read::compound(tag)?;
    Ok(DamageReduction {
        horizontal_blocking_angle: match read::field(entries, "horizontal_blocking_angle") {
            Some(tag) => positive_f32(read::float(tag)?)?,
            None => 90.0,
        },
        damage_types: read::field(entries, "type")
            .map(|tag| read::holder_set(Registry::DAMAGE_TYPE, tag))
            .transpose()?,
        base: read::float(read::required(entries, "base")?)?,
        factor: read::float(read::required(entries, "factor")?)?,
    })
}

fn decode_item_damage(tag: &Tag) -> Result<ItemDamageFunction> {
    let entries = read::compound(tag)?;
    Ok(ItemDamageFunction {
        threshold: non_negative_f32(read::float(read::required(entries, "threshold")?)?)?,
        base: read::float(read::required(entries, "base")?)?,
        factor: read::float(read::required(entries, "factor")?)?,
    })
}

fn decode_piercing(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::PiercingWeapon {
        deals_knockback: opt_bool(entries, "deals_knockback", true)?,
        dismounts: opt_bool(entries, "dismounts", false)?,
        sound: read::field(entries, "sound")
            .map(decode_sound)
            .transpose()?,
        hit_sound: read::field(entries, "hit_sound")
            .map(decode_sound)
            .transpose()?,
    })
}

fn decode_kinetic(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::KineticWeapon {
        contact_cooldown_ticks: opt_i32(entries, "contact_cooldown_ticks", 10, 0, i32::MAX)?,
        delay_ticks: opt_i32(entries, "delay_ticks", 0, 0, i32::MAX)?,
        dismount_conditions: read::field(entries, "dismount_conditions")
            .map(decode_condition)
            .transpose()?,
        knockback_conditions: read::field(entries, "knockback_conditions")
            .map(decode_condition)
            .transpose()?,
        damage_conditions: read::field(entries, "damage_conditions")
            .map(decode_condition)
            .transpose()?,
        forward_movement: match read::field(entries, "forward_movement") {
            Some(tag) => read::float(tag)?,
            None => 0.0,
        },
        damage_multiplier: match read::field(entries, "damage_multiplier") {
            Some(tag) => read::float(tag)?,
            None => 1.0,
        },
        sound: read::field(entries, "sound")
            .map(decode_sound)
            .transpose()?,
        hit_sound: read::field(entries, "hit_sound")
            .map(decode_sound)
            .transpose()?,
    })
}

fn decode_condition(tag: &Tag) -> Result<KineticCondition> {
    let entries = read::compound(tag)?;
    Ok(KineticCondition {
        max_duration_ticks: non_negative_i32(read::int(read::required(
            entries,
            "max_duration_ticks",
        )?)?)?,
        min_speed: match read::field(entries, "min_speed") {
            Some(tag) => read::float(tag)?,
            None => 0.0,
        },
        min_relative_speed: match read::field(entries, "min_relative_speed") {
            Some(tag) => read::float(tag)?,
            None => 0.0,
        },
    })
}

fn decode_string_map(tag: &Tag) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for (key, value) in read::compound(tag)? {
        let text = read::string(value)?.to_owned();
        if let Some((_, existing)) = out.iter_mut().find(|(name, _)| name == key) {
            *existing = text;
        } else {
            out.push((key.clone(), text));
        }
    }
    Ok(out)
}

fn decode_sound(tag: &Tag) -> Result<Holder<SoundEvent>> {
    read::holder(Registry::SOUND_EVENT, tag, |tag| {
        let entries = read::compound(tag)?;
        Ok(SoundEvent {
            location: read::identifier(read::required(entries, "sound_id")?)?,
            fixed_range: read::field(entries, "range")
                .map(|tag| positive_f32(read::float(tag)?))
                .transpose()?,
        })
    })
}

fn decode_consume(tag: &Tag, depth: u32) -> Result<ConsumeEffect> {
    let entries = read::compound(tag)?;
    let name = read::identifier(read::required(entries, "type")?)?;
    let id = read::name_id(Registry::CONSUME_EFFECT_TYPE, &name)
        .ok_or_else(|| Error::new(format!("no consume effect {name}")))?;
    let _ = depth;
    match id {
        consume_effect_type::APPLY_EFFECTS => Ok(ConsumeEffect::ApplyEffects {
            effects: read::list(read::required(entries, "effects")?)?
                .iter()
                .map(|item| decode_mob_effect(item, 0))
                .collect::<Result<_>>()?,
            probability: opt_f32(entries, "probability", 1.0, 0.0, 1.0)?,
        }),
        consume_effect_type::REMOVE_EFFECTS => Ok(ConsumeEffect::RemoveEffects {
            effects: read::holder_set(Registry::MOB_EFFECT, read::required(entries, "effects")?)?,
        }),
        consume_effect_type::CLEAR_ALL_EFFECTS => {
            unit(tag)?;
            Ok(ConsumeEffect::ClearAllEffects)
        }
        consume_effect_type::TELEPORT_RANDOMLY => Ok(ConsumeEffect::TeleportRandomly {
            diameter: match read::field(entries, "diameter") {
                Some(tag) => positive_f32(read::float(tag)?)?,
                None => 16.0,
            },
            directional_particles: opt_bool(entries, "directional_particles", true)?,
        }),
        consume_effect_type::PLAY_SOUND => Ok(ConsumeEffect::PlaySound {
            sound: decode_sound(read::required(entries, "sound")?)?,
        }),
        other => Err(Error::new(format!("unknown consume effect {other}"))),
    }
}

fn decode_mob_effect(tag: &Tag, depth: u32) -> Result<MobEffectInstance> {
    let entries = read::compound(tag)?;
    Ok(MobEffectInstance {
        effect: read::registry_id(Registry::MOB_EFFECT, read::required(entries, "id")?)?,
        details: decode_details(entries, depth)?,
    })
}

fn decode_details(entries: &[(String, Tag)], depth: u32) -> Result<EffectDetails> {
    if depth >= HIDDEN_LIMIT {
        return Err(Error::new("too many hidden mob effects"));
    }
    let show_particles = opt_bool(entries, "show_particles", true)?;
    let show_icon = match read::field(entries, "show_icon") {
        Some(tag) => read::bool_value(tag)?,
        None => show_particles,
    };
    let hidden = match read::field(entries, "hidden_effect") {
        Some(tag) => Some(Box::new(decode_details(read::compound(tag)?, depth + 1)?)),
        None => None,
    };
    Ok(EffectDetails {
        amplifier: match read::field(entries, "amplifier") {
            Some(tag) => unsigned_byte(tag)?,
            None => 0,
        },
        duration: opt_i32(entries, "duration", 0, i32::MIN, i32::MAX)?,
        ambient: opt_bool(entries, "ambient", false)?,
        show_particles,
        show_icon,
        hidden_effect: hidden,
    })
}

fn decode_templates(tag: &Tag, limit: usize, depth: u32) -> Result<Vec<ItemStackTemplate>> {
    let items = read::list(tag)?;
    if items.len() > limit {
        return Err(Error::new(format!("item list is longer than {limit}")));
    }
    items
        .iter()
        .map(|item| stack::decode_template(item, depth))
        .collect()
}

fn decode_potion(tag: &Tag, depth: u32) -> Result<ComponentValue> {
    if let Tag::String(_) = tag {
        return Ok(ComponentValue::PotionContents {
            potion: Some(read::registry_id(Registry::POTION, tag)?),
            custom_color: None,
            custom_effects: Vec::new(),
            custom_name: None,
        });
    }
    let entries = read::compound(tag)?;
    let _ = depth;
    Ok(ComponentValue::PotionContents {
        potion: read::field(entries, "potion")
            .map(|tag| read::registry_id(Registry::POTION, tag))
            .transpose()?,
        custom_color: read::field(entries, "custom_color")
            .map(read::int)
            .transpose()?,
        custom_effects: match read::field(entries, "custom_effects") {
            Some(tag) => read::list(tag)?
                .iter()
                .map(|item| decode_mob_effect(item, 0))
                .collect::<Result<_>>()?,
            None => Vec::new(),
        },
        custom_name: read::field(entries, "custom_name")
            .map(|tag| read::string(tag).map(str::to_owned))
            .transpose()?,
    })
}

fn decode_stew(tag: &Tag) -> Result<Vec<StewEffect>> {
    read::list(tag)?
        .iter()
        .map(|item| {
            let entries = read::compound(item)?;
            Ok(StewEffect {
                effect: read::registry_id(Registry::MOB_EFFECT, read::required(entries, "id")?)?,
                duration: opt_i32(entries, "duration", 160, i32::MIN, i32::MAX)?,
            })
        })
        .collect()
}

fn decode_writable(tag: &Tag) -> Result<Vec<Filterable<String>>> {
    let entries = read::compound(tag)?;
    let pages = match read::field(entries, "pages") {
        Some(tag) => read::list(tag)?,
        None => Vec::new(),
    };
    if pages.len() > BOOK_PAGES {
        return Err(Error::new("writable book has more than 100 pages"));
    }
    pages
        .iter()
        .map(|page| decode_filterable_string(page, PAGE_CHARS))
        .collect()
}

fn decode_filterable_string(tag: &Tag, max: usize) -> Result<Filterable<String>> {
    if let Tag::Compound(entries) = tag
        && read::field(entries, "raw").is_some()
    {
        return Ok(Filterable {
            raw: limited(read::string(read::required(entries, "raw")?)?, max)?,
            filtered: read::field(entries, "filtered")
                .map(|value| limited(read::string(value)?, max))
                .transpose()?,
        });
    }
    Ok(Filterable {
        raw: limited(read::string(tag)?, max)?,
        filtered: None,
    })
}

fn decode_filterable_text(tag: &Tag, depth: u32) -> Result<Filterable<Component>> {
    let _ = depth;
    if let Tag::Compound(entries) = tag
        && read::field(entries, "raw").is_some()
    {
        return Ok(Filterable {
            raw: text::decode(read::required(entries, "raw")?)?,
            filtered: read::field(entries, "filtered")
                .map(text::decode)
                .transpose()?,
        });
    }
    Ok(Filterable {
        raw: text::decode(tag)?,
        filtered: None,
    })
}

fn decode_written(tag: &Tag, depth: u32) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    let pages = match read::field(entries, "pages") {
        Some(tag) => read::list(tag)?
            .iter()
            .map(|page| decode_filterable_text(page, depth))
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    Ok(ComponentValue::WrittenBookContent {
        title: decode_filterable_string(read::required(entries, "title")?, TITLE_CHARS)?,
        author: read::string(read::required(entries, "author")?)?.to_owned(),
        generation: opt_i32(entries, "generation", 0, 0, 3)?,
        pages,
        resolved: opt_bool(entries, "resolved", false)?,
    })
}

fn decode_trim(tag: &Tag, depth: u32) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::Trim {
        material: decode_trim_material(read::required(entries, "material")?, depth)?,
        pattern: decode_trim_pattern(read::required(entries, "pattern")?, depth)?,
    })
}

fn decode_trim_material(tag: &Tag, depth: u32) -> Result<Holder<TrimMaterial>> {
    let _ = depth;
    read::holder(Registry::TRIM_MATERIAL, tag, |tag| {
        let entries = read::compound(tag)?;
        Ok(TrimMaterial {
            palette_id: read::identifier(read::required(entries, "palette_id")?)?,
            description: text::decode(read::required(entries, "description")?)?,
        })
    })
}

fn decode_trim_pattern(tag: &Tag, depth: u32) -> Result<Holder<TrimPattern>> {
    let _ = depth;
    read::holder(Registry::TRIM_PATTERN, tag, |tag| {
        let entries = read::compound(tag)?;
        Ok(TrimPattern {
            asset_id: read::identifier(read::required(entries, "asset_id")?)?,
            description: text::decode(read::required(entries, "description")?)?,
            decal: read::bool_value(read::required(entries, "decal")?)?,
        })
    })
}

fn decode_typed(tag: &Tag, registry: Registry) -> Result<TypedEntityData> {
    let entries = read::compound(tag)?;
    Ok(TypedEntityData {
        kind: read::registry_id(registry, read::required(entries, "id")?)?,
        tag: entries
            .iter()
            .filter(|(key, _)| key != "id")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    })
}

fn decode_instrument(tag: &Tag, depth: u32) -> Result<Holder<Instrument>> {
    let _ = depth;
    read::holder(Registry::INSTRUMENT, tag, |tag| {
        let entries = read::compound(tag)?;
        Ok(Instrument {
            sound: decode_sound(read::required(entries, "sound_event")?)?,
            use_duration: non_negative_f32(read::float(read::required(entries, "use_duration")?)?)?,
            range: positive_f32(read::float(read::required(entries, "range")?)?)?,
            durability_damage: opt_i32(entries, "durability_damage", 0, 0, i32::MAX)?,
            description: text::decode(read::required(entries, "description")?)?,
        })
    })
}

fn decode_lodestone(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    let target = read::field(entries, "target")
        .map(|tag| {
            let fields = read::compound(tag)?;
            Ok(GlobalPos {
                dimension: read::identifier(read::required(fields, "dimension")?)?,
                pos: decode_pos(read::required(fields, "pos")?)?,
            })
        })
        .transpose()?;
    Ok(ComponentValue::LodestoneTracker {
        target,
        tracked: opt_bool(entries, "tracked", true)?,
    })
}

fn decode_explosion(tag: &Tag) -> Result<FireworkExplosion> {
    let entries = read::compound(tag)?;
    Ok(FireworkExplosion {
        shape: read::enum_id(
            mc_bamboo_data::component_enums::names::FIREWORK_SHAPE,
            read::required(entries, "shape")?,
        )?,
        colors: optional_int_list(entries, "colors")?,
        fade_colors: optional_int_list(entries, "fade_colors")?,
        has_trail: opt_bool(entries, "has_trail", false)?,
        has_twinkle: opt_bool(entries, "has_twinkle", false)?,
    })
}

fn optional_int_list(entries: &[(String, Tag)], name: &str) -> Result<Vec<i32>> {
    match read::field(entries, name) {
        Some(tag) => read::list(tag)?.iter().map(read::int).collect(),
        None => Ok(Vec::new()),
    }
}

fn decode_fireworks(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    let explosions = match read::field(entries, "explosions") {
        Some(tag) => read::list(tag)?,
        None => Vec::new(),
    };
    if explosions.len() > FIREWORK_LIMIT {
        return Err(Error::new("fireworks have more than 256 explosions"));
    }
    Ok(ComponentValue::Fireworks {
        flight_duration: match read::field(entries, "flight_duration") {
            Some(tag) => unsigned_byte(tag)?,
            None => 0,
        },
        explosions: explosions
            .iter()
            .map(decode_explosion)
            .collect::<Result<_>>()?,
    })
}

pub(super) fn decode_profile(tag: &Tag) -> Result<ResolvableProfile> {
    if let Tag::String(name) = tag {
        return Ok(ResolvableProfile {
            profile: Err(PartialProfile {
                name: Some(player_name(name)?),
                id: None,
                properties: Vec::new(),
            }),
            skin_patch: SkinPatch {
                body: None,
                cape: None,
                elytra: None,
                slim: None,
            },
        });
    }
    let entries = read::compound(tag)?;
    let name = read::field(entries, "name")
        .map(|tag| player_name(read::string(tag)?))
        .transpose()?;
    let id = read::field(entries, "id").map(decode_uuid).transpose()?;
    let properties = match read::field(entries, "properties") {
        Some(tag) => decode_properties(tag)?,
        None => Vec::new(),
    };
    let profile = if let (Some(name), Some(id)) = (name.clone(), id) {
        Ok(GameProfile {
            id,
            name,
            properties,
        })
    } else {
        Err(PartialProfile {
            name,
            id,
            properties,
        })
    };
    Ok(ResolvableProfile {
        profile,
        skin_patch: decode_skin(entries)?,
    })
}

fn decode_properties(tag: &Tag) -> Result<Vec<Property>> {
    if let Tag::List(items) = tag {
        if items.len() > PROPERTY_LIMIT {
            return Err(Error::new("profile has more than 16 properties"));
        }
        return items.iter().map(decode_property).collect();
    }
    let entries = read::compound(tag)?;
    if entries.len() > PROPERTY_LIMIT {
        return Err(Error::new("profile has more than 16 properties"));
    }
    let mut properties = Vec::new();
    for (name, value) in entries {
        let name = limited(name, PROPERTY_NAME)?;
        for item in &read::list(value)? {
            properties.push(Property {
                name: name.clone(),
                value: limited(read::string(item)?, PROPERTY_VALUE)?,
                signature: None,
            });
        }
    }
    Ok(properties)
}

fn decode_property(tag: &Tag) -> Result<Property> {
    let entries = read::compound(tag)?;
    Ok(Property {
        name: limited(
            read::string(read::required(entries, "name")?)?,
            PROPERTY_NAME,
        )?,
        value: limited(
            read::string(read::required(entries, "value")?)?,
            PROPERTY_VALUE,
        )?,
        signature: read::field(entries, "signature")
            .map(|tag| limited(read::string(tag)?, PROPERTY_SIGNATURE))
            .transpose()?,
    })
}

fn decode_skin(entries: &[(String, Tag)]) -> Result<SkinPatch> {
    let model = match read::field(entries, "model") {
        Some(tag) => match read::string(tag)? {
            "slim" => Some(true),
            "wide" => Some(false),
            other => return Err(Error::new(format!("unknown player model {other}"))),
        },
        None => None,
    };
    Ok(SkinPatch {
        body: read::field(entries, "texture")
            .map(read::identifier)
            .transpose()?,
        cape: read::field(entries, "cape")
            .map(read::identifier)
            .transpose()?,
        elytra: read::field(entries, "elytra")
            .map(read::identifier)
            .transpose()?,
        slim: model,
    })
}

fn decode_banner(tag: &Tag, depth: u32) -> Result<Vec<BannerLayer>> {
    read::list(tag)?
        .iter()
        .map(|item| decode_banner_layer(item, depth))
        .collect()
}

fn decode_banner_layer(tag: &Tag, depth: u32) -> Result<BannerLayer> {
    let entries = read::compound(tag)?;
    Ok(BannerLayer {
        pattern: decode_banner_pattern(read::required(entries, "pattern")?, depth)?,
        color: dye(read::required(entries, "color")?)?,
    })
}

fn decode_banner_pattern(tag: &Tag, _depth: u32) -> Result<Holder<BannerPattern>> {
    read::holder(Registry::BANNER_PATTERN, tag, |tag| {
        let entries = read::compound(tag)?;
        Ok(BannerPattern {
            asset_id: read::identifier(read::required(entries, "asset_id")?)?,
            translation_key: read::string(read::required(entries, "translation_key")?)?.to_owned(),
        })
    })
}

fn decode_pot(tag: &Tag, depth: u32) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::PotDecorations {
        back: optional_template(entries, "back", depth)?,
        left: optional_template(entries, "left", depth)?,
        right: optional_template(entries, "right", depth)?,
        front: optional_template(entries, "front", depth)?,
    })
}

fn optional_template(
    entries: &[(String, Tag)],
    name: &str,
    depth: u32,
) -> Result<Option<ItemStackTemplate>> {
    read::field(entries, name)
        .map(|tag| stack::decode_template(tag, depth))
        .transpose()
}

fn decode_container(tag: &Tag, depth: u32) -> Result<Vec<Option<ItemStackTemplate>>> {
    let mut slots = Vec::new();
    for item in &read::list(tag)? {
        let entries = read::compound(item)?;
        let index = usize::try_from(write::range_i32(
            read::int(read::required(entries, "slot")?)?,
            0,
            255,
        )?)
        .map_err(|_| Error::new("container slot is negative"))?;
        let stack = stack::decode_template(read::required(entries, "item")?, depth)?;
        if slots.len() <= index {
            slots.resize_with(index + 1, || None);
        }
        slots[index] = Some(stack);
    }
    Ok(slots)
}

fn decode_bees(tag: &Tag) -> Result<Vec<BeeOccupant>> {
    read::list(tag)?
        .iter()
        .map(|item| {
            let entries = read::compound(item)?;
            Ok(BeeOccupant {
                entity_data: decode_typed(
                    read::required(entries, "entity_data")?,
                    Registry::ENTITY_TYPE,
                )?,
                ticks_in_hive: read::int(read::required(entries, "ticks_in_hive")?)?,
                min_ticks_in_hive: read::int(read::required(entries, "min_ticks_in_hive")?)?,
            })
        })
        .collect()
}

fn decode_resolvable_int(tag: &Tag) -> Result<ResolvableInt> {
    if let Tag::String(_) = tag {
        return Ok(ResolvableInt::Reference(read::identifier(tag)?));
    }
    Ok(ResolvableInt::Constant(read::int(tag)?))
}

fn decode_resolvable_float(tag: &Tag) -> Result<ResolvableFloat> {
    if let Tag::String(_) = tag {
        return Ok(ResolvableFloat::Reference(read::identifier(tag)?));
    }
    Ok(ResolvableFloat::Constant(read::float(tag)?))
}

fn decode_fuel(
    tag: &Tag,
    name: &str,
    build: impl FnOnce(ResolvableInt, ResolvableFloat) -> ComponentValue,
) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(build(
        decode_resolvable_int(read::required(entries, name)?)?,
        decode_resolvable_float(read::required(entries, "speed_multiplier")?)?,
    ))
}

fn decode_visibility(tag: &Tag) -> Result<ComponentValue> {
    let entries = read::compound(tag)?;
    Ok(ComponentValue::MobVisibility {
        targeting_entity_types: read::holder_set(
            Registry::ENTITY_TYPE,
            read::required(entries, "targeting_entity_types")?,
        )?,
        visibility: in_f32(
            read::float(read::required(entries, "visibility")?)?,
            0.0,
            10.0,
        )?,
    })
}

fn decode_sign(tag: &Tag, depth: u32) -> Result<SignText> {
    let entries = read::compound(tag)?;
    let messages = decode_sign_lines(read::required(entries, "messages")?, depth)?;
    let filtered = match read::field(entries, "filtered_messages") {
        Some(tag) => Some(decode_sign_lines(tag, depth)?),
        None => None,
    };
    Ok(SignText {
        messages,
        filtered_messages: filtered,
        color: match read::field(entries, "color") {
            Some(tag) => dye(tag)?,
            None => 15,
        },
        has_glowing_text: match read::field(entries, "has_glowing_text") {
            Some(tag) => read::bool_value(tag)?,
            None => false,
        },
    })
}

fn decode_sign_lines(tag: &Tag, _depth: u32) -> Result<Vec<Component>> {
    let lines = read::list(tag)?;
    if lines.len() != 4 {
        return Err(Error::new("sign text does not have 4 lines"));
    }
    lines.iter().map(text::decode).collect()
}

fn decode_swing(tag: &Tag) -> Result<SwingAnimation> {
    let entries = read::compound(tag)?;
    Ok(SwingAnimation {
        kind: match read::field(entries, "type") {
            Some(tag) => read::enum_id(
                mc_bamboo_data::component_enums::names::SWING_ANIMATION_TYPE,
                tag,
            )?,
            None => 1,
        },
        duration: opt_i32(entries, "duration", 6, 0, i32::MAX)?,
    })
}
