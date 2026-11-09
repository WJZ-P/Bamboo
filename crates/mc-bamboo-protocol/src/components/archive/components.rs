//! 每一种数据组件的存档 `Codec`。
//!
//! 类型还是网络编解码那一套。这里只补官方 `DataComponents` 里 `persistent` 的那份
//! `Codec`：字段名、默认值省略、`Either` 先试哪一支、持有者写成注册名还是内联。
//! 没有 `persistent` 的三种（`creative_slot_lock`、`additional_trade_cost`、
//! `map_post_processing`）编码直接失败。
//!
//! The persistent `Codec` of every data component.
//!
//! The values are the network types. This module only adds the archive shape:
//! field names, omitted defaults, which `Either` branch is tried first, and
//! whether a holder is a registry name or an inline value. The three components
//! without `persistent` fail to encode.

use mc_bamboo_data::component_enums::{names, sparse};
use mc_bamboo_nbt::Tag;

use super::{
    dynamic::Dyn,
    read::{Error, Result},
    records::*,
    stack, text,
    write::{self, Fields},
};
use crate::components::{ComponentValue, cursor::Registry};

/// 把一个组件的存档标签按官方 `Codec` 解一遍再编回去。
///
/// Decode one component's archive tag with the official `Codec` and encode it again.
pub(crate) fn transcode_persistent(kind: i32, tag: &Tag) -> Result<Tag> {
    Ok(encode_value(&decode_value(kind, tag, 0)?, 0)?.to_tag())
}

/// 存档标签按官方 `Codec` 解出来以后，和这份组件值是不是同一个。
///
/// Whether an archive tag decodes to this component value through the official `Codec`.
pub fn same_decoded(kind: i32, expected: &Tag, actual: &ComponentValue) -> bool {
    decode_value(kind, expected, 0).is_ok_and(|decoded| &decoded == actual)
}

/// 把一个类型化的组件值按官方 `Codec` 编成存档中间值。
///
/// Encodes a typed component value into the persistent intermediate form.
pub fn encode_value(value: &ComponentValue, depth: u32) -> Result<Dyn> {
    crate::stack_hop::guard(|| encode_value_frame(value, depth))
}

fn encode_value_frame(value: &ComponentValue, depth: u32) -> Result<Dyn> {
    encode_early(value, depth)
        .or_else(|| encode_items(value, depth))
        .or_else(|| encode_late(value, depth))
        .unwrap_or_else(|| Err(Error::new("component has no archive codec")))
}

/// 解一个组件的存档标签，供战利品表这类的调用方用。
///
/// Decodes one component's persistent tag for callers such as the loot-table bridge.
pub fn decode_value(kind: i32, tag: &Tag, depth: u32) -> Result<ComponentValue> {
    if let Some(value) = decode_early(kind, tag, depth)? {
        return Ok(value);
    }
    if let Some(value) = decode_items(kind, tag, depth)? {
        return Ok(value);
    }
    decode_late(kind, tag, depth)?
        .ok_or_else(|| Error::new(format!("component {kind} has no archive codec")))
}

fn encode_early(value: &ComponentValue, depth: u32) -> Option<Result<Dyn>> {
    use ComponentValue::{
        AttackRange, AttributeModifiers, BlockState, BlocksAttacks, BucketEntityData, CanBreak,
        CanPlaceOn, Consumable, CustomData, CustomModelData, CustomName, Damage, DamageResistant,
        DamageType, DeathProtection, Dye, DyedColor, Enchantable, EnchantmentGlintOverride,
        Enchantments, EntityData, Equippable, Food, Glider, IntangibleProjectile, ItemModel,
        ItemName, KineticWeapon, Lore, MaxDamage, MaxStackSize, MinimumAttackCharge,
        PiercingWeapon, Rarity, RepairCost, Repairable, StoredEnchantments, Tool, TooltipDisplay,
        TooltipStyle, Unbreakable, UseCooldown, UseEffects, UseRemainder, Weapon,
    };
    Some(match value {
        CustomData(entries) | BucketEntityData(entries) => {
            let tag = Tag::Compound(entries.clone());
            let encoded = Dyn::from_tag(&tag);
            // 临时标签不能靠自动 `Drop` 拆。
            //
            // The temporary tag must not use automatic `Drop`.
            Tag::release(tag);
            Ok(encoded)
        }
        MaxStackSize(value) => int_dyn(write::range_i32(*value, 1, 99)),
        MaxDamage(value) => int_dyn(positive_i32(*value)),
        Damage(value) | RepairCost(value) => int_dyn(non_negative_i32(*value)),
        Unbreakable | Glider | IntangibleProjectile => Ok(Dyn::empty_map()),
        UseEffects {
            can_sprint,
            interact_vibrations,
            speed_multiplier,
        } => encode_use_effects(*can_sprint, *interact_vibrations, *speed_multiplier),
        CustomName(component) | ItemName(component) => text::encode(component, depth),
        MinimumAttackCharge(value) => float_dyn(in_f32(*value, 0.0, 1.0)),
        DamageType(id) => id_name(Registry::DAMAGE_TYPE, *id),
        ItemModel(name) | TooltipStyle(name) => Ok(write::identifier(name)),
        Lore(lines) => encode_lore(lines, depth),
        Rarity(id) => write::enum_name(names::RARITY, *id),
        Enchantments(entries) | StoredEnchantments(entries) => encode_enchantments(entries),
        CanPlaceOn(predicates) | CanBreak(predicates) => encode_adventure(predicates, depth),
        AttributeModifiers(entries) => encode_attributes(entries, depth),
        CustomModelData(data) => encode_model(data),
        TooltipDisplay {
            hide_tooltip,
            hidden_components,
        } => encode_tooltip(*hide_tooltip, hidden_components),
        EnchantmentGlintOverride(value) => Ok(Dyn::Bool(*value)),
        Food {
            nutrition,
            saturation,
            can_always_eat,
        } => encode_food(*nutrition, *saturation, *can_always_eat),
        Consumable {
            consume_seconds,
            animation,
            sound,
            has_consume_particles,
            on_consume_effects,
        } => encode_consumable(
            *consume_seconds,
            *animation,
            sound,
            *has_consume_particles,
            on_consume_effects,
            depth,
        ),
        UseRemainder(template) => stack::encode_template(template, depth),
        UseCooldown {
            seconds,
            cooldown_group,
        } => encode_cooldown(*seconds, cooldown_group),
        DamageResistant(types) => encode_holder_set_field("types", Registry::DAMAGE_TYPE, types),
        Tool {
            rules,
            default_mining_speed,
            damage_per_block,
            can_destroy_blocks_in_creative,
        } => encode_tool(
            rules,
            *default_mining_speed,
            *damage_per_block,
            *can_destroy_blocks_in_creative,
        ),
        Weapon {
            item_damage_per_attack,
            disable_blocking_for_seconds,
        } => encode_weapon(*item_damage_per_attack, *disable_blocking_for_seconds),
        AttackRange {
            min_reach,
            max_reach,
            min_creative_reach,
            max_creative_reach,
            hitbox_margin,
            mob_factor,
        } => encode_attack_range(
            *min_reach,
            *max_reach,
            *min_creative_reach,
            *max_creative_reach,
            *hitbox_margin,
            *mob_factor,
        ),
        Enchantable(value) => encode_positive_field("value", *value),
        Equippable(value) => encode_equippable(value, depth),
        Repairable(items) => encode_holder_set_field("items", Registry::ITEM, items),
        DeathProtection(effects) => encode_effect_list("death_effects", effects, depth),
        BlocksAttacks {
            block_delay_seconds,
            disable_cooldown_scale,
            damage_reductions,
            item_damage,
            bypassed_by,
            block_sound,
            disabled_sound,
        } => encode_blocks_attacks(
            *block_delay_seconds,
            *disable_cooldown_scale,
            damage_reductions,
            item_damage,
            bypassed_by,
            block_sound,
            disabled_sound,
        ),
        PiercingWeapon {
            deals_knockback,
            dismounts,
            sound,
            hit_sound,
        } => encode_piercing(*deals_knockback, *dismounts, sound, hit_sound),
        KineticWeapon {
            contact_cooldown_ticks,
            delay_ticks,
            dismount_conditions,
            knockback_conditions,
            damage_conditions,
            forward_movement,
            damage_multiplier,
            sound,
            hit_sound,
        } => encode_kinetic(
            (*contact_cooldown_ticks, *delay_ticks),
            (dismount_conditions, knockback_conditions, damage_conditions),
            (*forward_movement, *damage_multiplier),
            (sound, hit_sound),
        ),
        BlockState(entries) => Ok(Dyn::Map(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), Dyn::String(value.clone())))
                .collect(),
        )),
        Dye(id) => write::enum_name(names::DYE_COLOR, *id),
        DyedColor(color) => Ok(Dyn::Int(*color)),
        EntityData(data) => encode_typed(data, Registry::ENTITY_TYPE),
        _ => return None,
    })
}

fn encode_items(value: &ComponentValue, depth: u32) -> Option<Result<Dyn>> {
    use ComponentValue::{
        BannerPatterns, BaseColor, Bees, BlockEntityData, BlockTransformer, BreakSound,
        BrewingFuel, BundleContents, ChargedProjectiles, Compostable, Container, ContainerLoot,
        CookingFuel, DebugStickState, FireworkExplosion, Fireworks, Instrument, JukeboxPlayable,
        Lock, LodestoneTracker, MapDecorations, MapId, MobVisibility, NoteBlockSound,
        OminousBottleAmplifier, PotDecorations, PotionContents, PotionDurationScale, Profile,
        ProvidesBannerPatterns, ProvidesTrimMaterial, Recipes, SignTextBack, SignTextFront,
        SulfurCubeContent, SuspiciousStewEffects, Trim, VillagerFood, Waxed, WritableBookContent,
        WrittenBookContent,
    };
    Some(match value {
        BlockTransformer(id) => id_name(Registry::BLOCK_TRANSFORMER, *id),
        VillagerFood(value) => encode_positive_field("nutrition", *value),
        MapId(value) => Ok(Dyn::Int(*value)),
        MapDecorations(tag) | DebugStickState(tag) => Ok(Dyn::from_tag(tag)),
        ChargedProjectiles(items) => encode_templates(items, 1024, depth),
        BundleContents(items) => encode_templates(items, usize::MAX, depth),
        PotionContents {
            potion,
            custom_color,
            custom_effects,
            custom_name,
        } => encode_potion(
            *potion,
            *custom_color,
            custom_effects,
            custom_name.as_deref(),
            depth,
        ),
        PotionDurationScale(value) => float_dyn(non_negative_f32(*value)),
        SuspiciousStewEffects(effects) => encode_stew(effects),
        WritableBookContent(pages) => encode_writable(pages),
        WrittenBookContent {
            title,
            author,
            generation,
            pages,
            resolved,
        } => encode_written(title, author, *generation, pages, *resolved, depth),
        Trim { material, pattern } => encode_trim(material, pattern, depth),
        BlockEntityData(data) => encode_typed(data, Registry::BLOCK_ENTITY_TYPE),
        Instrument(holder) => encode_instrument(holder, depth),
        ProvidesTrimMaterial(holder) => encode_trim_material(holder, depth),
        OminousBottleAmplifier(value) => int_dyn(write::range_i32(*value, 0, 4)),
        JukeboxPlayable(holder) => write::holder_fixed(Registry::JUKEBOX_SONG, holder),
        ProvidesBannerPatterns(set) => write::holder_set(Registry::BANNER_PATTERN, set),
        Recipes(tag) => encode_recipes(tag),
        LodestoneTracker { target, tracked } => encode_lodestone(target.as_ref(), *tracked),
        FireworkExplosion(explosion) => encode_explosion(explosion),
        Fireworks {
            flight_duration,
            explosions,
        } => encode_fireworks(*flight_duration, explosions),
        Profile(profile) => encode_profile(profile, depth),
        NoteBlockSound(name) => Ok(write::identifier(name)),
        BannerPatterns(layers) => encode_banner(layers, depth),
        BaseColor(id) => write::enum_name(names::DYE_COLOR, *id),
        PotDecorations {
            back,
            left,
            right,
            front,
        } => encode_pot(back, left, right, front, depth),
        Container(slots) => encode_container(slots, depth),
        Bees(bees) => encode_bees(bees),
        SulfurCubeContent(template) => stack::encode_template(template, depth),
        Lock(tag) => encode_lock(tag, depth),
        ContainerLoot(tag) => encode_loot(tag),
        BreakSound(holder) => encode_sound(holder),
        Compostable(layers) => {
            encode_resolvable_int(layers).map(|layers| Fields::new().put("layers", layers).build())
        }
        CookingFuel {
            burn_time,
            speed_multiplier,
        } => encode_fuel("burn_time", burn_time, speed_multiplier),
        BrewingFuel {
            uses,
            speed_multiplier,
        } => encode_fuel("uses", uses, speed_multiplier),
        MobVisibility {
            targeting_entity_types,
            visibility,
        } => encode_visibility(targeting_entity_types, *visibility),
        SignTextFront(text) | SignTextBack(text) => encode_sign(text, depth),
        Waxed => Ok(Dyn::empty_map()),
        _ => return None,
    })
}

fn encode_late(value: &ComponentValue, depth: u32) -> Option<Result<Dyn>> {
    let _ = depth;
    use ComponentValue::{
        AdditionalTradeCost, AttackAnimation, AxolotlVariant, CatCollar, CatSoundVariant,
        CatVariant, ChickenSoundVariant, ChickenVariant, CowSoundVariant, CowVariant,
        CreativeSlotLock, CushionColor, FoxVariant, FrogVariant, HorseVariant, InteractAnimation,
        LlamaVariant, MapPostProcessing, MooshroomVariant, PaintingVariant, ParrotVariant,
        PigSoundVariant, PigVariant, ProvidesPotteryPattern, RabbitVariant, SalmonSize, SheepColor,
        ShulkerColor, TropicalFishBaseColor, TropicalFishPattern, TropicalFishPatternColor,
        VillagerVariant, WolfCollar, WolfSoundVariant, WolfVariant, ZombieNautilusVariant,
    };
    Some(match value {
        CreativeSlotLock | AdditionalTradeCost(_) | MapPostProcessing(_) => {
            Err(Error::new("component is not persistent"))
        }
        AttackAnimation(value) | InteractAnimation(value) => encode_swing(value),
        VillagerVariant(id) => id_name(Registry::VILLAGER_TYPE, *id),
        WolfVariant(id) => id_name(Registry::WOLF_VARIANT, *id),
        WolfSoundVariant(id) => id_name(Registry::WOLF_SOUND_VARIANT, *id),
        WolfCollar(id)
        | CatCollar(id)
        | SheepColor(id)
        | ShulkerColor(id)
        | CushionColor(id)
        | TropicalFishBaseColor(id)
        | TropicalFishPatternColor(id) => write::enum_name(names::DYE_COLOR, *id),
        FoxVariant(id) => write::enum_name(names::FOX_VARIANT, *id),
        SalmonSize(id) => write::enum_name(names::SALMON_SIZE, *id),
        ParrotVariant(id) => write::enum_name(names::PARROT_VARIANT, *id),
        TropicalFishPattern(id) => sparse_name(
            sparse::TROPICAL_FISH_PATTERN,
            names::TROPICAL_FISH_PATTERN,
            *id,
        ),
        MooshroomVariant(id) => write::enum_name(names::MOOSHROOM_VARIANT, *id),
        RabbitVariant(id) => sparse_name(sparse::RABBIT_VARIANT, names::RABBIT_VARIANT, *id),
        PigVariant(id) => id_name(Registry::PIG_VARIANT, *id),
        PigSoundVariant(id) => id_name(Registry::PIG_SOUND_VARIANT, *id),
        CowVariant(id) => id_name(Registry::COW_VARIANT, *id),
        CowSoundVariant(id) => id_name(Registry::COW_SOUND_VARIANT, *id),
        ChickenVariant(id) => id_name(Registry::CHICKEN_VARIANT, *id),
        ChickenSoundVariant(id) => id_name(Registry::CHICKEN_SOUND_VARIANT, *id),
        ZombieNautilusVariant(id) => id_name(Registry::ZOMBIE_NAUTILUS_VARIANT, *id),
        FrogVariant(id) => id_name(Registry::FROG_VARIANT, *id),
        HorseVariant(id) => write::enum_name(names::HORSE_VARIANT, *id),
        PaintingVariant(holder) => write::holder_fixed(Registry::PAINTING_VARIANT, holder),
        LlamaVariant(id) => write::enum_name(names::LLAMA_VARIANT, *id),
        AxolotlVariant(id) => write::enum_name(names::AXOLOTL_VARIANT, *id),
        CatVariant(id) => id_name(Registry::CAT_VARIANT, *id),
        CatSoundVariant(id) => id_name(Registry::CAT_SOUND_VARIANT, *id),
        ProvidesPotteryPattern(id) => id_name(Registry::DECORATED_POT_PATTERN, *id),
        _ => return None,
    })
}
