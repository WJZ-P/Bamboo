//! 生物的存档 NBT：官方 `Entity.saveWithoutId` / `load` 加上 `LivingEntity`、`Mob`、
//! `AgeableMob`、`Animal`、`Cow` 各层的 `addAdditionalSaveData` / `readAdditionalSaveData`。
//!
//! 读写的键、类型、缺省值和「只在非默认时写」的条件逐条照官方字节码。官方有、这里
//! 不建模的键（装备、拴绳、`data`、睡觉位置……）读进来放进
//! [`Mob::passthrough`]，存盘时原样写回；不认识的键和官方一样丢掉。
//! `active_effects` 按官方 `MobEffectInstance.CODEC` 建模读写。
//! `/summon` 的 NBT 也走这里（官方 `SummonCommand` 用 `EntityType.loadEntityRecursive`）。
//!
//! Mob archive NBT, following vanilla `Entity.saveWithoutId` / `load` and each
//! layer's additional save data. Vanilla keys that are not modelled go into
//! [`Mob::passthrough`] and are written back verbatim; unknown keys are dropped
//! like vanilla does. `/summon` NBT goes through here too.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::registries::{attribute, mob_effect};
use mc_bamboo_nbt::Tag;

use super::{
    CowSoundVariant, CowVariant, Mob, MobType,
    attributes::{Attribute, Modifier, Operation},
    effects::{EffectInstance, EffectMap},
    living,
};
use crate::{
    archive::tag::{self, Error, Result, put},
    entity::{Entity, EntityRow, KindPublic, KindState},
};

/// 官方有、这里不建模，原样保留写回的键。
///
/// Vanilla keys kept verbatim and written back.
const PASSTHROUGH: &[&str] = &[
    "leash",
    "DeathLootTable",
    "data",
    "HasVisualFire",
    "locator_bar_icon",
    "sleeping_pos",
    "current_explosion_impact_pos",
];

/// `MobEffectInstance.CODEC` 的一项：`id` 加 `Details` 的六个字段（可递归的
/// `hidden_effect`）。官方 `optionalFieldOf` 在等于缺省值时省键：
/// `amplifier` 用 UNSIGNED_BYTE 编（BYTE 标签）、`duration` 用 INT、
/// `ambient` 缺省 false、`show_particles` 缺省 true；`show_icon` 是
/// `Optional<Boolean>` 但 getter 永远返回 `Optional.of(...)`，所以总写。
/// 读档时缺省值由 `Details.create` 补齐（`show_icon` 缺省等于 `show_particles`）。
///
/// One entry of `MobEffectInstance.CODEC`. Vanilla's `optionalFieldOf`
/// omits default values; `show_icon` is always written.
pub fn effect_tag(instance: &EffectInstance) -> Tag {
    let mut out = vec![(
        "id".to_owned(),
        Tag::String(
            mob_effect::name_of(instance.effect)
                .unwrap_or("minecraft:speed")
                .to_owned(),
        ),
    )];
    put_details(&mut out, instance);
    Tag::Compound(out)
}

/// `Details.MAP_CODEC` 的字段（递归的 `hidden_effect` 没有 `id`）。
fn effect_details_tag(instance: &EffectInstance) -> Tag {
    let mut out = Vec::new();
    put_details(&mut out, instance);
    Tag::Compound(out)
}

fn put_details(out: &mut Vec<(String, Tag)>, instance: &EffectInstance) {
    // `ExtraCodecs.UNSIGNED_BYTE` 在 NBT 里是 ByteTag。
    if instance.amplifier != 0 {
        put(
            out,
            "amplifier",
            Tag::Byte(instance.amplifier.min(255) as i8),
        );
    }
    if instance.duration != 0 {
        put(out, "duration", Tag::Int(instance.duration));
    }
    if instance.ambient {
        put(out, "ambient", Tag::Byte(1));
    }
    if !instance.visible {
        put(out, "show_particles", Tag::Byte(0));
    }
    put(out, "show_icon", Tag::Byte(i8::from(instance.show_icon)));
    if let Some(hidden) = &instance.hidden {
        put(out, "hidden_effect", effect_details_tag(hidden));
    }
}

/// `active_effects` 列表，按效果 ID 升序（官方按 HashMap 序，这里取确定顺序）。
pub fn effects_tag(effects: &EffectMap) -> Tag {
    Tag::List(effects.entries().iter().map(effect_tag).collect())
}

/// 读一项 `MobEffectInstance.CODEC`。未知效果 ID 官方会解码失败整表丢弃，
/// 这里按「跳过这一项」处理，坏数据不会 panic。
///
/// Reads one `MobEffectInstance.CODEC` entry. Vanilla fails the whole list on
/// an unknown effect id; here bad entries are skipped so disk data cannot panic.
pub fn read_effect(tag: &Tag) -> Option<EffectInstance> {
    let id = tag::string_or(tag, "id").and_then(mob_effect::by_name)?;
    let mut instance = EffectInstance {
        effect: id,
        duration: tag::int_or(tag, "duration", 0),
        amplifier: amplifier_field(tag),
        ambient: tag::bool_or(tag, "ambient", false),
        visible: tag::bool_or(tag, "show_particles", true),
        show_icon: match tag::field(tag, "show_icon") {
            Some(value) => tag::java_i64(value).unwrap_or(0) != 0,
            None => tag::bool_or(tag, "show_particles", true),
        },
        hidden: None,
    };
    instance.hidden = tag::field(tag, "hidden_effect")
        .map(|hidden| read_hidden_effect(hidden, id))
        .map(Box::new);
    Some(instance)
}

/// `ExtraCodecs.UNSIGNED_BYTE` 把 0..255 存成 Java 的 byte（-128..127），
/// 读回来要按无符号还原。官方写 `255` 时 NBT 里是 `Byte(-1)`。
///
/// `ExtraCodecs.UNSIGNED_BYTE` stores 0..255 in a Java byte; read it back
/// as unsigned (255 is written as `Byte(-1)`).
fn amplifier_field(tag: &Tag) -> i32 {
    match tag::field(tag, "amplifier") {
        Some(Tag::Byte(value)) => i32::from(*value as u8),
        Some(value) => tag::java_i32(value).unwrap_or(0).clamp(0, 255),
        None => 0,
    }
}

fn read_hidden_effect(tag: &Tag, effect: i32) -> EffectInstance {
    let mut instance = EffectInstance::new(effect, tag::int_or(tag, "duration", 0));
    instance.amplifier = amplifier_field(tag);
    instance.ambient = tag::bool_or(tag, "ambient", false);
    instance.visible = tag::bool_or(tag, "show_particles", true);
    instance.show_icon = match tag::field(tag, "show_icon") {
        Some(value) => tag::java_i64(value).unwrap_or(0) != 0,
        None => instance.visible,
    };
    instance.hidden = tag::field(tag, "hidden_effect")
        .map(|hidden| read_hidden_effect(hidden, effect))
        .map(Box::new);
    instance
}

/// 读实体存档里的 `active_effects` 字段。不是列表时按空表处理。
pub fn read_effects(tag: &Tag) -> EffectMap {
    tag::field(tag, "active_effects")
        .map(read_effects_tag)
        .unwrap_or_default()
}

/// 读一个已经是 `active_effects` 列表的标签（玩家存档层直接持有这个标签）。
///
/// Reads a tag that already is the `active_effects` list.
pub fn read_effects_tag(tag: &Tag) -> EffectMap {
    let mut map = EffectMap::new();
    if let Tag::List(entries) = tag {
        let instances: Vec<EffectInstance> = entries.iter().filter_map(read_effect).collect();
        map.load(instances);
    }
    map
}

/// 写一个生物：官方 `Entity.save` 的结果（含 `id`）。
pub fn write_mob(row: &EntityRow) -> Result<Tag> {
    let public = &row.public;
    let state = &row.state;
    let (KindPublic::Mob(_), KindState::Mob(mob)) = (&public.kind, &state.kind) else {
        return Err(Error::new("mismatched mob entity halves"));
    };
    let mut out = Vec::new();
    put(&mut out, "id", Tag::String(mob.mob_type.name().to_owned()));
    // ---- Entity.saveWithoutId ----
    put(&mut out, "Pos", vec3_tag(public.position));
    put(&mut out, "Motion", vec3_tag(public.delta));
    put(
        &mut out,
        "Rotation",
        Tag::List(vec![Tag::Float(public.yaw), Tag::Float(mob.x_rot)]),
    );
    put(&mut out, "fall_distance", Tag::Double(state.fall_distance));
    put(
        &mut out,
        "Fire",
        Tag::Short(state.remaining_fire_ticks as i16),
    );
    put(&mut out, "Air", Tag::Short(mob.air_supply as i16));
    put(&mut out, "OnGround", Tag::Byte(i8::from(public.on_ground)));
    put(
        &mut out,
        "Invulnerable",
        Tag::Byte(i8::from(mob.invulnerable)),
    );
    put(&mut out, "PortalCooldown", Tag::Int(mob.portal_cooldown));
    if mob.invulnerable_time > 0 {
        put(
            &mut out,
            "invulnerable_time",
            Tag::Int(mob.invulnerable_time),
        );
    }
    put(&mut out, "UUID", uuid_tag(&mob.uuid));
    if let Some(name) = &mob.custom_name {
        put(&mut out, "CustomName", name.clone());
    }
    if mob.custom_name_visible {
        put(&mut out, "CustomNameVisible", Tag::Byte(1));
    }
    if mob.silent {
        put(&mut out, "Silent", Tag::Byte(1));
    }
    if mob.no_gravity {
        put(&mut out, "NoGravity", Tag::Byte(1));
    }
    if mob.glowing {
        put(&mut out, "Glowing", Tag::Byte(1));
    }
    if mob.ticks_frozen > 0 {
        put(&mut out, "TicksFrozen", Tag::Int(mob.ticks_frozen));
    }
    if !mob.tags.is_empty() {
        put(
            &mut out,
            "Tags",
            Tag::List(mob.tags.iter().cloned().map(Tag::String).collect()),
        );
    }
    // ---- LivingEntity ----
    put(&mut out, "Health", Tag::Float(mob.health));
    put(&mut out, "HurtTime", Tag::Short(mob.hurt_time as i16));
    put(&mut out, "DeathTime", Tag::Short(mob.death_time as i16));
    put(&mut out, "AbsorptionAmount", Tag::Float(mob.absorption));
    put(
        &mut out,
        "current_impulse_context_reset_grace_time",
        Tag::Int(mob.impulse_grace_time),
    );
    put(&mut out, "attributes", attributes_tag(mob));
    if !mob.effects.is_empty() {
        put(&mut out, "active_effects", effects_tag(&mob.effects));
    }
    put(&mut out, "FallFlying", Tag::Byte(i8::from(mob.fall_flying)));
    put(
        &mut out,
        "Brain",
        super::brain::nbt::write(mob.brain.as_deref()),
    );
    if let Some(uuid) = &mob.last_hurt_by_player_ref {
        put(&mut out, "last_hurt_by_player", uuid_tag(uuid));
        put(
            &mut out,
            "last_hurt_by_player_memory_time",
            Tag::Int(mob.last_hurt_by_player_memory_time),
        );
    }
    if let Some(uuid) = &mob.last_hurt_by_mob_ref {
        put(&mut out, "last_hurt_by_mob", uuid_tag(uuid));
        put(
            &mut out,
            "ticks_since_last_hurt_by_mob",
            Tag::Int(
                state
                    .tick_count
                    .wrapping_sub(mob.last_hurt_by_mob_timestamp),
            ),
        );
    }
    // `LivingEntity`：装备非空才写 `equipment`（`EntityEquipment.CODEC`）。
    if !mob.equipment.is_empty() {
        put(&mut out, "equipment", mob.equipment.equipment_tag());
    }
    // ---- Mob ----
    put(
        &mut out,
        "CanPickUpLoot",
        Tag::Byte(i8::from(mob.can_pick_up_loot)),
    );
    put(
        &mut out,
        "PersistenceRequired",
        Tag::Byte(i8::from(mob.persistence_required)),
    );
    // `Mob`：掉落几率不是 `DropChances.DEFAULT` 才写，等于缺省值的槽省略。
    if !mob.equipment.default_drop_chances() {
        put(&mut out, "drop_chances", mob.equipment.drop_chances_tag());
    }
    if let Some((pos, radius)) = mob.home {
        put(&mut out, "home_radius", Tag::Int(radius));
        put(&mut out, "home_pos", block_pos_tag(pos));
    }
    put(&mut out, "LeftHanded", Tag::Byte(i8::from(mob.left_handed)));
    if mob.loot_table_seed != 0 {
        put(
            &mut out,
            "DeathLootTableSeed",
            Tag::Long(mob.loot_table_seed),
        );
    }
    if mob.no_ai {
        put(&mut out, "NoAI", Tag::Byte(1));
    }
    // ---- AgeableMob / Animal / Cow ----
    if mob.mob_type.is_ageable() && mob.mob_type.can_be_a_baby() {
        put(&mut out, "Age", Tag::Int(mob.age));
        put(&mut out, "ForcedAge", Tag::Int(mob.forced_age));
        put(&mut out, "AgeLocked", Tag::Byte(i8::from(mob.age_locked)));
    }
    // 史莱姆和鱿鱼一支是 `AgeableMob` 但不是 `Animal`，没有求爱的两个键。
    // Slimes and the squid branch are `AgeableMob`s but not `Animal`s, so they
    // lack the two love keys.
    if mob.mob_type.is_animal() && mob.mob_type != MobType::Slime {
        put(&mut out, "InLove", Tag::Int(mob.in_love));
        if let Some(uuid) = &mob.love_cause {
            put(&mut out, "LoveCause", uuid_tag(uuid));
        }
    }
    // ---- AbstractCubeMob ----
    if mob.mob_type == MobType::Slime {
        put(&mut out, "Size", Tag::Int(mob.cube.size - 1));
        put(
            &mut out,
            "wasOnGround",
            Tag::Byte(i8::from(mob.cube.was_on_ground)),
        );
    }
    // ---- Skeleton / Bogged ----
    if mob.mob_type == MobType::Skeleton {
        // `ConversionTracker.addAdditionalSaveData`：`isAfflicted` 是存档这一刻的
        // `isInPowderSnow`。
        // `ConversionTracker.addAdditionalSaveData`: `isAfflicted` is `isInPowderSnow` at
        // save time.
        mob.freeze_conversion.write(
            &mut out,
            super::conversion::STRAY_CONVERSION_KEY,
            super::conversion::FREEZING_KEY,
            mob.in_powder_snow,
        );
    }
    if mob.mob_type == MobType::Bogged {
        put(&mut out, "sheared", Tag::Byte(i8::from(mob.bogged_sheared)));
    }
    // ---- PatrollingMonster / Raider / Pillager ----
    if mob.mob_type.is_raider() {
        mob.raider.save(&mut out);
    }
    if mob.mob_type == MobType::Pillager {
        super::pillager::save(mob, &mut out);
    }
    if mob.mob_type == MobType::GlowSquid {
        put(
            &mut out,
            "DarkTicksRemaining",
            Tag::Int(mob.squid.dark_ticks),
        );
    }
    // ---- Turtle.addAdditionalSaveData ----
    if mob.mob_type == MobType::Turtle {
        put(&mut out, "home_pos", block_pos_tag(mob.turtle.home_pos));
        put(&mut out, "has_egg", Tag::Byte(i8::from(mob.turtle.has_egg)));
    }
    if mob.mob_type == MobType::Rabbit {
        put(&mut out, "RabbitType", Tag::Int(mob.rabbit.variant.id()));
        put(
            &mut out,
            "MoreCarrotTicks",
            Tag::Int(mob.rabbit.more_carrot_ticks),
        );
    }
    // ---- Fox.addAdditionalSaveData ----
    if mob.mob_type == MobType::Fox {
        let trusted = mob.fox.trusted.iter().flatten().map(uuid_tag).collect();
        put(&mut out, "Trusted", Tag::List(trusted));
        let fox = &mob.fox;
        put(&mut out, "Sleeping", Tag::Byte(i8::from(fox.is_sleeping())));
        put(&mut out, "Type", Tag::String(fox.variant.name().to_owned()));
        let sitting = fox.has(super::fox::FLAG_SITTING);
        put(&mut out, "Sitting", Tag::Byte(i8::from(sitting)));
        let crouching = fox.has(super::fox::FLAG_CROUCHING);
        put(&mut out, "Crouching", Tag::Byte(i8::from(crouching)));
    }
    if super::tame::is_tamable(mob.mob_type) {
        write_tame(&mut out, mob);
    }
    if mob.mob_type == MobType::Wolf {
        write_wolf(&mut out, mob);
    }
    // ---- Sheep / Chicken ----
    if mob.mob_type == MobType::Sheep {
        put(
            &mut out,
            "Sheared",
            Tag::Byte(i8::from(super::sheep::is_sheared(mob))),
        );
        put(
            &mut out,
            "Color",
            Tag::Byte(super::sheep::color(mob).id() as i8),
        );
    }
    if mob.mob_type == MobType::Armadillo {
        put(
            &mut out,
            "state",
            Tag::String(mob.armadillo.state.name().to_owned()),
        );
        put(&mut out, "scute_time", Tag::Int(mob.armadillo.scute_time));
    }
    if mob.mob_type == MobType::Frog {
        // `Frog.addAdditionalSaveData`：`VariantUtils.writeVariant`。
        // `Frog.addAdditionalSaveData`: `VariantUtils.writeVariant`.
        put(
            &mut out,
            "variant",
            Tag::String(mob.frog.variant.key().to_owned()),
        );
    }
    if mob.mob_type == MobType::Goat {
        // `Goat.addAdditionalSaveData`。
        // `Goat.addAdditionalSaveData`.
        let goat = mob.goat;
        put(
            &mut out,
            "IsScreamingGoat",
            Tag::Byte(i8::from(goat.screaming)),
        );
        put(&mut out, "HasLeftHorn", Tag::Byte(i8::from(goat.left_horn)));
        put(
            &mut out,
            "HasRightHorn",
            Tag::Byte(i8::from(goat.right_horn)),
        );
    }
    if mob.mob_type == MobType::Mooshroom {
        super::mooshroom::write_nbt(mob, &mut out);
    }
    if mob.mob_type == MobType::PolarBear {
        super::polar_bear::write_nbt(mob, &mut out);
    }
    if mob.mob_type == MobType::Panda {
        super::panda::write_nbt(mob, &mut out);
    }
    if mob.mob_type == MobType::Chicken {
        put(&mut out, "IsChickenJockey", Tag::Byte(0));
        put(&mut out, "EggLayTime", Tag::Int(mob.egg_time));
    }
    if matches!(mob.mob_type, MobType::Cow | MobType::Pig | MobType::Chicken) {
        put(
            &mut out,
            "variant",
            Tag::String(mob.variant.name().to_owned()),
        );
        let sound = match mob.mob_type {
            MobType::Pig => mob.pig_sound_variant.name(),
            MobType::Chicken => mob.chicken_sound_variant.name(),
            _ => mob.sound_variant.name(),
        };
        put(&mut out, "sound_variant", Tag::String(sound.to_owned()));
    }
    super::equine::save(mob, &mut out)?;
    // ---- Zombie ----
    if mob.mob_type.uses_baby_metadata() {
        put(&mut out, "IsBaby", Tag::Byte(i8::from(mob.zombie_baby)));
        put(
            &mut out,
            "CanBreakDoors",
            Tag::Byte(i8::from(mob.can_break_doors)),
        );
        // `Zombie.addAdditionalSaveData` 的 `drowningTracker`：`isAfflicted` 是存档这一刻的
        // `isEyeInFluid(WATER)`。
        //
        // The `drowningTracker` part of `Zombie.addAdditionalSaveData`: `isAfflicted` is
        // `isEyeInFluid(WATER)` at save time.
        let afflicted = living::eye_in_fluid(mob, mc_bamboo_data::tags::fluid_tags::WATER);
        mob.drown_conversion.write(
            &mut out,
            super::drowned::DROWNED_CONVERSION_KEY,
            super::drowned::IN_WATER_TIME_KEY,
            afflicted,
        );
    }
    if mob.mob_type == MobType::Creeper {
        super::creeper::write_nbt(mob, &mut out);
    }
    if mob.mob_type == MobType::Enderman {
        super::enderman::write_nbt(mob, &mut out);
    }
    // ---- AbstractFish / Salmon / TropicalFish / Pufferfish ----
    if super::fish::is_fish(mob.mob_type) {
        put(
            &mut out,
            "FromBucket",
            Tag::Byte(i8::from(mob.fish.from_bucket)),
        );
    }
    if mob.mob_type == MobType::Tadpole {
        // `Tadpole.addAdditionalSaveData`。
        put(&mut out, "Age", Tag::Int(mob.tadpole_age));
        put(&mut out, "AgeLocked", Tag::Byte(i8::from(mob.age_locked)));
    }
    match mob.mob_type {
        MobType::Salmon => put(
            &mut out,
            "type",
            Tag::String(super::fish::salmon_variant_name(mob.fish.variant).to_owned()),
        ),
        MobType::TropicalFish => put(&mut out, "Variant", Tag::Int(mob.fish.variant)),
        MobType::Pufferfish => put(&mut out, "PuffState", Tag::Int(mob.fish.puff_state)),
        _ => {}
    }
    for (key, value) in &mob.passthrough {
        out.retain(|(existing, _)| existing != key);
        out.push((key.clone(), value.clone()));
    }
    Ok(Tag::Compound(out))
}

/// 读一个生物；不是这一轮支持的生物时返回 `None`，交回调用方照旧处理。
pub fn read_mob(tag: &Tag, id: &str, uid: i32) -> Result<Option<EntityRow>> {
    let Some(kind) = MobType::by_name(id) else {
        return Ok(None);
    };
    let mut entity = Entity::by_mob_type(kind, Vec3::default());
    entity.id = uid;
    // 读盘的实体不是新建的：构造时的随机数（UUID、朝向）不取，缺 `UUID` 时全零。
    // A loaded entity is not freshly built: the constructor draws are skipped.
    load_into(&mut entity, tag)?;
    entity.with_mob(|_, mob| {
        // 读盘的生物随机源在放进世界时才定：大脑的洗牌种子和传感器延迟改用 UUID 算，
        // 不取任何随机源（官方这两样本来就不可复现）。
        // A loaded mob's random is set when it joins the world, so the brain's shuffle
        // seeds and sensor delays come from the UUID without drawing (vanilla's are not
        // reproducible anyway).
        if let Some(brain) = mob.brain.as_mut() {
            let seed = i64::from_be_bytes(mob.uuid[..8].try_into().unwrap_or_default())
                ^ i64::from_be_bytes(mob.uuid[8..].try_into().unwrap_or_default());
            brain.seed_shuffles(seed);
            brain.randomly_delay_start(&mut crate::entity::JavaRandom::new(seed));
        }
    });
    Ok(Some(entity.split()))
}

/// 官方 `Entity.load`：把 `tag` 读进一个已经构造好的牛。缺的键按官方缺省值，
/// 所以 `/summon` 不带 NBT 时朝向也会被设成 0（官方就是这样）。
///
/// Vanilla `Entity.load` into an already built cow. Missing keys take vanilla
/// defaults, so a bare `/summon` resets the facing to 0 just like vanilla.
pub fn load_into(entity: &mut Entity, tag: &Tag) -> Result<()> {
    let position = tag::vec3(tag, "Pos", [0.0; 3]);
    let motion = tag::vec3(tag, "Motion", [0.0; 3]);
    let rotation = tag::vec2(tag, "Rotation", [0.0; 2]);
    let clip = |value: f64| if value.abs() > 10.0 { 0.0 } else { value };
    let position = Vec3::new(
        position[0].clamp(-3.000_051_2e7, 3.000_051_2e7),
        position[1].clamp(-2.0e7, 2.0e7),
        position[2].clamp(-3.000_051_2e7, 3.000_051_2e7),
    );
    if !(position.x.is_finite() && position.y.is_finite() && position.z.is_finite()) {
        return Err(Error::new("Entity has invalid position"));
    }
    if !(rotation[0].is_finite() && rotation[1].is_finite()) {
        return Err(Error::new("Entity has invalid rotation"));
    }
    let loaded = entity.with_mob(|me, mob| {
        me.delta = Vec3::new(clip(motion[0]), clip(motion[1]), clip(motion[2]));
        me.position = position;
        me.old_position = position;
        me.y_rot = rotation[0];
        me.fall_distance = tag::double_or(tag, "fall_distance", 0.0);
        me.remaining_fire_ticks = i32::from(tag::short_or(tag, "Fire", 0));
        me.on_ground = tag::bool_or(tag, "OnGround", false);
        mob.x_rot = rotation[1];
        mob.y_rot_o = me.y_rot;
        mob.x_rot_o = mob.x_rot;
        mob.y_head_rot = if mob.mob_type == MobType::Goat {
            super::goat::clamped_head_rot(mob.y_body_rot, me.y_rot)
        } else {
            me.y_rot
        };
        mob.y_head_rot_o = me.y_rot;
        mob.y_body_rot = me.y_rot;
        mob.y_body_rot_o = me.y_rot;
        mob.air_supply = tag::int_or(tag, "Air", 300);
        mob.invulnerable = tag::bool_or(tag, "Invulnerable", false);
        mob.invulnerable_time = tag::int_or(tag, "invulnerable_time", 0);
        mob.portal_cooldown = tag::int_or(tag, "PortalCooldown", 0);
        if let Some(uuid) = uuid_field(tag, "UUID") {
            mob.uuid = uuid;
        }
        mob.custom_name = tag::field(tag, "CustomName").map(normalize_component);
        mob.custom_name_visible = tag::bool_or(tag, "CustomNameVisible", false);
        mob.silent = tag::bool_or(tag, "Silent", false);
        mob.no_gravity = tag::bool_or(tag, "NoGravity", false);
        mob.glowing = tag::bool_or(tag, "Glowing", false);
        mob.ticks_frozen = tag::int_or(tag, "TicksFrozen", 0);
        mob.tags = match tag::field(tag, "Tags") {
            Some(Tag::List(values)) => values
                .iter()
                .filter_map(|value| match value {
                    Tag::String(text) => Some(text.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        // `AbstractCubeMob.readAdditionalSaveData`：`setSize(Size + 1, false)` 在
        // `super.readAdditionalSaveData` 之前，后面读到的 `attributes` 和 `Health` 盖在它上面。
        // `AbstractCubeMob.readAdditionalSaveData`: `setSize(Size + 1, false)` runs ahead of
        // `super.readAdditionalSaveData`, so the saved `attributes` and `Health` land on top.
        if mob.mob_type == MobType::Slime {
            let size = tag::int_or(tag, "Size", 0).saturating_add(1);
            super::slime::set_size(me, mob, size, false);
            mob.cube.was_on_ground = tag::bool_or(tag, "wasOnGround", false);
        }
        // ---- LivingEntity.readAdditionalSaveData ----
        mob.absorption = tag::float_or(tag, "AbsorptionAmount", 0.0).max(0.0);
        if let Some(Tag::List(entries)) = tag::field(tag, "attributes") {
            apply_attributes(mob, entries);
        }
        // 官方先读 `attributes` 再读 `active_effects`，然后按效果重建修饰，
        // `Health` 才按新的最大血量夹。读盘不触发 `onEffectAdded` 的包。
        // Vanilla reads `attributes` then `active_effects`, rebuilds the
        // modifiers, and only then clamps `Health`; loading sends no packets.
        mob.effects = read_effects(tag);
        mob.effects.rebuild_attributes(&mut mob.attributes);
        let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
        mob.health = mc_bamboo_core::mth::clamp_f32(tag::float_or(tag, "Health", max), 0.0, max);
        mob.hurt_time = i32::from(tag::short_or(tag, "HurtTime", 0));
        mob.death_time = i32::from(tag::short_or(tag, "DeathTime", 0));
        mob.fall_flying = tag::bool_or(tag, "FallFlying", false);
        mob.last_hurt_by_player_ref = uuid_field(tag, "last_hurt_by_player");
        mob.last_hurt_by_player = None;
        mob.last_hurt_by_player_memory_time =
            tag::int_or(tag, "last_hurt_by_player_memory_time", 0);
        mob.last_hurt_by_mob_ref = uuid_field(tag, "last_hurt_by_mob");
        mob.last_hurt_by_mob = None;
        mob.last_hurt_by_mob_timestamp =
            me.tick_count
                .wrapping_sub(tag::int_or(tag, "ticks_since_last_hurt_by_mob", 0));
        mob.impulse_grace_time = tag::int_or(tag, "current_impulse_context_reset_grace_time", 0);
        // `equipment`：官方读失败时取部分结果，坏的条目丢掉、好的留下。
        mob.equipment.read_equipment(tag::field(tag, "equipment"));
        // ---- Mob.readAdditionalSaveData ----
        mob.can_pick_up_loot = tag::bool_or(tag, "CanPickUpLoot", false);
        mob.persistence_required = tag::bool_or(tag, "PersistenceRequired", false);
        mob.equipment
            .read_drop_chances(tag::field(tag, "drop_chances"));
        let radius = tag::int_or(tag, "home_radius", -1);
        mob.home = (radius >= 0).then(|| {
            let pos = tag::int_array(tag, "home_pos");
            let pos = if pos.len() == 3 {
                BlockPos::new(pos[0], pos[1], pos[2])
            } else {
                BlockPos::new(0, 0, 0)
            };
            (pos, radius)
        });
        // `LivingEntity.readAdditionalSaveData`：有 `Brain` 键就 `makeBrain(packed)`，
        // 传感器的起始延迟再从生物随机源取一遍。
        // `LivingEntity.readAdditionalSaveData`: a `Brain` key rebuilds the brain, drawing
        // the sensors' start delays from the mob's random again.
        if let Some(saved) = tag::field(tag, "Brain")
            && let Some(mut brain) = super::new_brain(mob.mob_type)
        {
            brain.seed_shuffles(me.random.state());
            brain.randomly_delay_start(&mut me.random);
            super::brain::nbt::read(&mut brain, saved);
            mob.brain = Some(brain);
        }
        mob.left_handed = tag::bool_or(tag, "LeftHanded", false);
        mob.loot_table_seed = tag::long_or(tag, "DeathLootTableSeed", 0);
        mob.no_ai = tag::bool_or(tag, "NoAI", false);
        // ---- AgeableMob / Animal / Cow ----
        if mob.mob_type.is_ageable() && mob.mob_type.can_be_a_baby() {
            living::set_age(me, mob, tag::int_or(tag, "Age", 0));
            mob.forced_age = tag::int_or(tag, "ForcedAge", 0);
            mob.age_locked = tag::bool_or(tag, "AgeLocked", false);
        }
        if mob.mob_type.is_animal() {
            mob.in_love = tag::int_or(tag, "InLove", 0);
            mob.love_cause = uuid_field(tag, "LoveCause");
        }
        // ---- Skeleton / Bogged ----
        if mob.mob_type == MobType::Skeleton {
            mob.freeze_conversion.read(
                tag,
                super::conversion::STRAY_CONVERSION_KEY,
                super::conversion::FREEZING_KEY,
            );
        }
        if mob.mob_type == MobType::Bogged {
            mob.bogged_sheared = tag::bool_or(tag, "sheared", false);
        }
        // ---- PatrollingMonster / Raider / Pillager ----
        if mob.mob_type.is_raider() {
            mob.raider = super::raider::Raider::load(tag);
        }
        if mob.mob_type == MobType::Pillager {
            super::pillager::load(mob, tag);
        }
        if mob.mob_type == MobType::GlowSquid {
            mob.squid.dark_ticks = tag::int_or(tag, "DarkTicksRemaining", 0);
        }
        // `Turtle.readAdditionalSaveData`：先读自己的家方块，再交给父类。
        // `Turtle.readAdditionalSaveData`: the home first, then the superclass.
        if mob.mob_type == MobType::Turtle {
            let pos = tag::int_array(tag, "home_pos");
            mob.turtle.home_pos = if pos.len() == 3 {
                BlockPos::new(pos[0], pos[1], pos[2])
            } else {
                me.block_position()
            };
            mob.turtle.has_egg = tag::bool_or(tag, "has_egg", false);
        }
        if mob.mob_type == MobType::Rabbit {
            mob.rabbit.variant = super::rabbit::Variant::from_id(tag::int_or(tag, "RabbitType", 0));
            mob.rabbit.more_carrot_ticks = tag::int_or(tag, "MoreCarrotTicks", 0);
        }
        if mob.mob_type == MobType::Fox {
            read_fox(mob, tag);
        }
        if super::tame::is_tamable(mob.mob_type) {
            read_tame(mob, tag);
        }
        if mob.mob_type == MobType::Wolf {
            read_wolf(mob, tag);
        }
        // ---- Sheep / Chicken ----
        if mob.mob_type == MobType::Sheep {
            super::sheep::set_sheared(mob, tag::bool_or(tag, "Sheared", false));
            let color = tag::int_or(tag, "Color", 0);
            super::sheep::set_color(mob, mc_bamboo_core::sign::DyeColor::from_id(color));
        }
        if mob.mob_type == MobType::Armadillo {
            read_armadillo(mob, tag);
        }
        if mob.mob_type == MobType::Frog {
            // `Frog.readAdditionalSaveData`：读不出品种就保留原来的。
            // `Frog.readAdditionalSaveData`: an unreadable variant keeps the current one.
            if let Some(variant) =
                tag::string_or(tag, "variant").and_then(super::frog::Variant::from_name)
            {
                mob.frog.variant = variant;
            }
        }
        if mob.mob_type == MobType::Goat {
            // `Goat.readAdditionalSaveData`：缺省不尖叫、两只角都在。
            // `Goat.readAdditionalSaveData`: not screaming and both horns by default.
            mob.goat.screaming = tag::bool_or(tag, "IsScreamingGoat", false);
            mob.goat.left_horn = tag::bool_or(tag, "HasLeftHorn", true);
            mob.goat.right_horn = tag::bool_or(tag, "HasRightHorn", true);
        }
        if mob.mob_type == MobType::Mooshroom {
            super::mooshroom::read_nbt(mob, tag);
        }
        if mob.mob_type == MobType::PolarBear {
            super::polar_bear::read_nbt(mob, tag);
        }
        if mob.mob_type == MobType::Panda {
            super::panda::read_nbt(mob, tag);
        }
        if mob.mob_type == MobType::Chicken {
            // `input.getInt("EggLayTime").ifPresent`：没有这个键时保留构造时抽的值。
            // A missing key keeps the constructor's roll.
            mob.egg_time = tag::int_or(tag, "EggLayTime", mob.egg_time);
        }
        if matches!(mob.mob_type, MobType::Cow | MobType::Pig | MobType::Chicken) {
            if let Some(variant) = tag::string_or(tag, "variant").and_then(|name| {
                if mob.mob_type != MobType::Cow && !name.contains(':') {
                    CowVariant::from_name(&format!("minecraft:{name}"))
                } else {
                    CowVariant::from_name(name)
                }
            }) {
                mob.variant = variant;
            }
            let sound = tag::string_or(tag, "sound_variant");
            if mob.mob_type == MobType::Chicken {
                if let Some(variant) = sound.and_then(super::chicken::SoundVariant::from_name) {
                    mob.chicken_sound_variant = variant;
                }
            } else if mob.mob_type == MobType::Pig {
                if let Some(variant) = sound.and_then(super::pig::SoundVariant::from_name) {
                    mob.pig_sound_variant = variant;
                }
            } else if let Some(variant) = sound.and_then(CowSoundVariant::from_name) {
                mob.sound_variant = variant;
            }
        }
        // ---- AbstractFish / Salmon / TropicalFish / Pufferfish ----
        if mob.mob_type == MobType::Tadpole {
            // `Tadpole.readAdditionalSaveData`；`setFromBucket` 是空的，桶标志不读。存档里
            // 年龄已经到了的，下一刻长一刻时就变。
            // `Tadpole.readAdditionalSaveData`; `setFromBucket` is empty, so the bucket flag
            // is not read. A saved age already past the limit converts on the next tick.
            mob.tadpole_age = tag::int_or(tag, "Age", 0);
            mob.age_locked = tag::bool_or(tag, "AgeLocked", false);
        } else if super::fish::is_fish(mob.mob_type) {
            mob.fish.from_bucket = tag::bool_or(tag, "FromBucket", false);
        }
        match mob.mob_type {
            MobType::Salmon => {
                let variant = tag::string_or(tag, "type")
                    .and_then(super::fish::salmon_variant_from_name)
                    .unwrap_or(1);
                mob.fish.variant = variant;
                super::fish::refresh_dimensions(me, mob);
            }
            MobType::TropicalFish => {
                mob.fish.variant = tag::int_or(tag, "Variant", 0);
            }
            MobType::Pufferfish => {
                mob.fish.puff_state = tag::int_or(tag, "PuffState", 0).min(2);
                super::fish::refresh_dimensions(me, mob);
            }
            _ => {}
        }
        super::equine::load(mob, tag);
        // ---- Zombie ----
        if mob.mob_type.uses_baby_metadata() {
            let baby = tag::bool_or(tag, "IsBaby", false);
            mob.set_zombie_baby(baby);
            let (width, height, eye) = mob.mob_type.dimensions(baby);
            me.dimensions = (width, height);
            mob.eye_height = eye;
            mob.can_break_doors = tag::bool_or(tag, "CanBreakDoors", false);
            if mob.can_break_doors {
                mob.navigation.flags.can_open_doors = true;
                mob.goals.insert_break_door();
            }
            mob.drown_conversion.read(
                tag,
                super::drowned::DROWNED_CONVERSION_KEY,
                super::drowned::IN_WATER_TIME_KEY,
            );
            mob.drowned_converting = mob.drown_conversion.converting;
        }
        if mob.mob_type == MobType::Creeper {
            super::creeper::read_nbt(mob, tag);
        }
        if mob.mob_type == MobType::Enderman {
            super::enderman::read_nbt(mob, tag);
        }
        mob.passthrough = match tag {
            Tag::Compound(entries) => entries
                .iter()
                .filter(|(key, _)| PASSTHROUGH.contains(&key.as_str()))
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
    });
    loaded.ok_or_else(|| Error::new("not a mob"))
}

/// `AttributeMap.apply`：逐条找实例（懒创建）、设基础值、加永久修饰符。
/// 牛没有的属性官方记一条警告跳过。
fn apply_attributes(mob: &mut Mob, entries: &[Tag]) {
    for entry in entries {
        let Some(id) = tag::string_or(entry, "id").and_then(attribute::by_name) else {
            continue;
        };
        let Some(instance) = mob.attributes.instance(id) else {
            continue;
        };
        if let Some(base) = tag::field(entry, "base").and_then(|value| tag::java_f64(value).ok()) {
            instance.base = base;
        }
        let Some(Tag::List(modifiers)) = tag::field(entry, "modifiers") else {
            continue;
        };
        for modifier in modifiers {
            let (Some(name), Some(operation)) = (
                tag::string_or(modifier, "id"),
                tag::string_or(modifier, "operation").and_then(operation_from_name),
            ) else {
                continue;
            };
            let amount = tag::double_or(modifier, "amount", 0.0);
            instance.modifiers.retain(|existing| existing.id != name);
            instance.modifiers.push(Modifier {
                id: name.to_owned(),
                amount,
                operation,
                permanent: true,
            });
        }
    }
}

/// `AttributeInstance.Packed.LIST_CODEC`：懒创建过的实例，`base` 总写，`modifiers`
/// 只写永久的、空时省掉。官方按哈希序遍历（每次开服都可能不同），这里按注册表 ID。
///
/// Instantiated attributes only; `base` always, permanent `modifiers` when any.
/// Vanilla iterates in identity-hash order; this writes in registry id order.
fn attributes_tag(mob: &Mob) -> Tag {
    let mut entries: Vec<&Attribute> = mob
        .attributes
        .entries
        .iter()
        .filter(|entry| entry.instantiated)
        .collect();
    entries.sort_by_key(|entry| entry.id);
    Tag::List(
        entries
            .into_iter()
            .map(|entry| {
                let mut out = vec![
                    (
                        "id".to_owned(),
                        Tag::String(attribute::name_of(entry.id).unwrap_or("").to_owned()),
                    ),
                    ("base".to_owned(), Tag::Double(entry.base)),
                ];
                let modifiers: Vec<Tag> = entry
                    .modifiers
                    .iter()
                    .filter(|modifier| modifier.permanent)
                    .map(|modifier| {
                        Tag::Compound(vec![
                            ("id".to_owned(), Tag::String(modifier.id.clone())),
                            ("amount".to_owned(), Tag::Double(modifier.amount)),
                            (
                                "operation".to_owned(),
                                Tag::String(operation_name(modifier.operation).to_owned()),
                            ),
                        ])
                    })
                    .collect();
                if !modifiers.is_empty() {
                    out.push(("modifiers".to_owned(), Tag::List(modifiers)));
                }
                Tag::Compound(out)
            })
            .collect(),
    )
}

const fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::AddValue => "add_value",
        Operation::AddMultipliedBase => "add_multiplied_base",
        Operation::AddMultipliedTotal => "add_multiplied_total",
    }
}

fn operation_from_name(name: &str) -> Option<Operation> {
    match name {
        "add_value" => Some(Operation::AddValue),
        "add_multiplied_base" => Some(Operation::AddMultipliedBase),
        "add_multiplied_total" => Some(Operation::AddMultipliedTotal),
        _ => None,
    }
}

/// `ComponentSerialization.CODEC` 写回时，只有 `text` 一个键的组件写成裸字符串。
///
/// A component that is only `{text: ...}` is written back as a bare string.
fn normalize_component(value: &Tag) -> Tag {
    if let Tag::Compound(entries) = value
        && let [(key, Tag::String(text))] = entries.as_slice()
        && key == "text"
    {
        return Tag::String(text.clone());
    }
    value.clone()
}

fn vec3_tag(value: Vec3) -> Tag {
    Tag::List(vec![
        Tag::Double(value.x),
        Tag::Double(value.y),
        Tag::Double(value.z),
    ])
}

/// `BlockPos.CODEC` 是三个 int 的数组。
fn block_pos_tag(pos: BlockPos) -> Tag {
    Tag::IntArray(vec![pos.x, pos.y, pos.z])
}

/// `UUIDUtil.CODEC`：四个 int，大端拆开。
pub(crate) fn uuid_tag(bytes: &[u8; 16]) -> Tag {
    let mut words = [0i32; 4];
    for (index, word) in words.iter_mut().enumerate() {
        let mut value = 0u32;
        for &byte in &bytes[index * 4..index * 4 + 4] {
            value = value << 8 | u32::from(byte);
        }
        *word = value as i32;
    }
    Tag::IntArray(words.to_vec())
}

/// `Fox.readAdditionalSaveData`：清空信任名单再逐个加回，最后 `setTargetGoals`。
///
/// `Fox.readAdditionalSaveData`: refill the trusted list, then `setTargetGoals`.
/// `Armadillo.readAdditionalSaveData`：`state` 缺省 `idle`，`scute_time` 没有时保留构造时抽的值。
///
/// `Armadillo.readAdditionalSaveData`: `state` defaults to `idle`; a missing
/// `scute_time` keeps the constructor's roll.
fn read_armadillo(mob: &mut super::Mob, tag: &Tag) {
    let state = tag::string_or(tag, "state")
        .and_then(super::armadillo::State::from_name)
        .unwrap_or_default();
    if mob.armadillo.state != state {
        mob.armadillo.state = state;
        mob.armadillo.in_state_ticks = 0;
    }
    mob.armadillo.scute_time = tag::int_or(tag, "scute_time", mob.armadillo.scute_time);
}

fn read_fox(mob: &mut super::Mob, tag: &Tag) {
    mob.fox.trusted = [None, None];
    if let Some(Tag::List(entries)) = tag::field(tag, "Trusted") {
        for entry in entries {
            if let Some(uuid) = uuid_words(entry) {
                mob.fox.add_trusted(uuid);
            }
        }
    }
    let fox = &mut mob.fox;
    fox.set(
        super::fox::FLAG_SLEEPING,
        tag::bool_or(tag, "Sleeping", false),
    );
    fox.variant = tag::string_or(tag, "Type")
        .and_then(super::fox::Variant::from_name)
        .unwrap_or_default();
    fox.set(
        super::fox::FLAG_SITTING,
        tag::bool_or(tag, "Sitting", false),
    );
    fox.set(
        super::fox::FLAG_CROUCHING,
        tag::bool_or(tag, "Crouching", false),
    );
    super::fox::set_target_goals(mob);
}

/// `TamableAnimal.addAdditionalSaveData`：`Owner`（有才写）和 `Sitting`。
fn write_tame(out: &mut Vec<(String, Tag)>, mob: &super::Mob) {
    if let Some(owner) = &mob.tame.owner {
        put(out, "Owner", uuid_tag(owner));
    }
    put(out, "Sitting", Tag::Byte(i8::from(mob.tame.ordered_to_sit)));
}

/// `Wolf.addAdditionalSaveData`：项圈（字节 ID）、品种、持续仇恨、叫声。
fn write_wolf(out: &mut Vec<(String, Tag)>, mob: &super::Mob) {
    let wolf = &mob.wolf;
    put(out, "CollarColor", Tag::Byte(wolf.collar as i8));
    put(out, "variant", Tag::String(wolf.variant_name().to_owned()));
    put(out, "anger_end_time", Tag::Long(wolf.anger_end_time));
    if let Some(target) = &wolf.angry_at {
        put(out, "angry_at", uuid_tag(target));
    }
    put(
        out,
        "sound_variant",
        Tag::String(wolf.sound_variant_name().to_owned()),
    );
}

/// `TamableAnimal.readAdditionalSaveData`：有主人就 `setTame(true, false)`，
/// 没有就 `setTame(false, true)`（带副作用，狼的最大生命回到 8）；坐姿跟着 `Sitting`。
/// 旧版按名字存的主人（`OldUsersConverter`）不转换。
///
/// `TamableAnimal.readAdditionalSaveData`; legacy name-based owners are not converted.
fn read_tame(mob: &mut super::Mob, tag: &Tag) {
    match uuid_field(tag, "Owner") {
        Some(owner) => {
            mob.tame.owner = Some(owner);
            super::tame::set_tame(mob, true, false);
        }
        None => {
            mob.tame.owner = None;
            super::tame::set_tame(mob, false, true);
        }
    }
    mob.tame.ordered_to_sit = tag::bool_or(tag, "Sitting", false);
    let sitting = mob.tame.ordered_to_sit;
    mob.tame.set_in_sitting_pose(sitting);
}

/// `Wolf.readAdditionalSaveData`。旧的 `AngerTime` 要用读盘时的游戏时刻换算，
/// 这里拿不到，按没有仇恨处理；`angry_at` 只记 UUID，不当场设目标。
///
/// `Wolf.readAdditionalSaveData`. Legacy `AngerTime` needs the game time and is
/// read as no anger; `angry_at` keeps the UUID without setting a target.
fn read_wolf(mob: &mut super::Mob, tag: &Tag) {
    if let Some(variant) = tag::string_or(tag, "variant").and_then(super::wolf::variant_id) {
        mob.wolf.variant = variant;
    }
    mob.wolf.collar = match tag::field(tag, "CollarColor") {
        Some(Tag::Byte(id)) if (0..16).contains(id) => i32::from(*id),
        Some(Tag::Byte(_)) => 0,
        _ => 14,
    };
    mob.wolf.anger_end_time = tag::long_or(tag, "anger_end_time", -1);
    mob.wolf.angry_at = uuid_field(tag, "angry_at");
    mob.wolf.angry_at_uid = None;
    if let Some(sound) =
        tag::string_or(tag, "sound_variant").and_then(super::wolf::sound_variant_id)
    {
        mob.wolf.sound_variant = sound;
    }
}

fn uuid_words(tag: &Tag) -> Option<[u8; 16]> {
    match tag {
        Tag::IntArray(words) if words.len() == 4 => {
            let mut out = [0u8; 16];
            for (index, word) in words.iter().enumerate() {
                out[index * 4..index * 4 + 4].copy_from_slice(&(*word as u32).to_be_bytes());
            }
            Some(out)
        }
        _ => None,
    }
}

pub(super) fn uuid_field(tag: &Tag, key: &str) -> Option<[u8; 16]> {
    match tag::field(tag, key) {
        Some(Tag::IntArray(words)) if words.len() == 4 => {
            let mut out = [0u8; 16];
            for (index, word) in words.iter().enumerate() {
                out[index * 4..index * 4 + 4].copy_from_slice(&(*word as u32).to_be_bytes());
            }
            Some(out)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(instances: Vec<EffectInstance>) -> EffectMap {
        let mut map = EffectMap::new();
        map.load(instances);
        let tag = Tag::Compound(vec![("active_effects".to_owned(), effects_tag(&map))]);
        read_effects(&tag)
    }

    #[test]
    fn effect_archive_round_trips_all_fields() {
        let mut instance =
            EffectInstance::with_flags(mob_effect::REGENERATION, 1234, 3, true, false);
        instance.show_icon = false;
        instance.hidden = Some(Box::new(EffectInstance::with_amplifier(
            mob_effect::REGENERATION,
            600,
            1,
        )));
        let mut infinite = EffectInstance::with_amplifier(mob_effect::SPEED, -1, 255);
        infinite.ambient = true;
        let map = round_trip(vec![instance.clone(), infinite.clone()]);
        assert_eq!(map.entries(), vec![infinite, instance]);
    }

    #[test]
    fn effect_archive_defaults_match_the_vanilla_codec() {
        let tag = effect_tag(&EffectInstance::new(mob_effect::SPEED, 100));
        assert_eq!(tag::string_or(&tag, "id"), Some("minecraft:speed"));
        // 官方 codec 省掉等于缺省值的键：`amplifier` / `ambient` /
        // `show_particles` 都不写，`duration` 和 `show_icon` 写。
        assert!(tag::field(&tag, "amplifier").is_none());
        assert!(tag::field(&tag, "ambient").is_none());
        assert!(tag::field(&tag, "show_particles").is_none());
        assert_eq!(tag::int_or(&tag, "duration", -1), 100);
        assert!(tag::bool_or(&tag, "show_icon", false));
        // 非缺省值时写 Byte 标签，读回来按任意数值标签取值。
        let tagged = effect_tag(&EffectInstance::with_flags(
            mob_effect::REGENERATION,
            1234,
            2,
            true,
            false,
        ));
        assert!(matches!(
            tag::field(&tagged, "amplifier"),
            Some(Tag::Byte(2))
        ));
        assert_eq!(tag::int_or(&tagged, "amplifier", -1), 2);
        assert!(tag::bool_or(&tagged, "ambient", false));
        assert!(!tag::bool_or(&tagged, "show_particles", true));
        // 改坏确认：把 show_icon 写成 false，往返会丢掉图标。
        let mut broken = EffectInstance::new(mob_effect::SPEED, 100);
        broken.show_icon = false;
        assert!(!tag::bool_or(&effect_tag(&broken), "show_icon", true));
        assert!(tag::bool_or(&tag, "show_icon", false));
    }

    #[test]
    fn unknown_effect_ids_and_bad_tags_are_skipped() {
        let tag = Tag::Compound(vec![(
            "active_effects".to_owned(),
            Tag::List(vec![
                Tag::Compound(vec![
                    (
                        "id".to_owned(),
                        Tag::String("minecraft:not_an_effect".to_owned()),
                    ),
                    ("duration".to_owned(), Tag::Int(100)),
                ]),
                Tag::String("not a compound".to_owned()),
                effect_tag(&EffectInstance::new(mob_effect::HASTE, 42)),
            ]),
        )]);
        let map = read_effects(&tag);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(mob_effect::HASTE).unwrap().duration, 42);
    }

    /// `equipment` 跟在 `LivingEntity` 的键后面、`drop_chances` 跟在 `PersistenceRequired`
    /// 后面；读回来装备和几率都一样，属性修饰符要等第一刻的装备比较才挂上。
    #[test]
    fn equipment_and_drop_chances_round_trip_in_the_vanilla_key_order() {
        use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

        let mut zombie = Entity::zombie(Vec3::new(0.5, 64.0, 0.5));
        let helmet = ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1);
        if let Some(mob) = zombie.kind.mob_mut() {
            mob.equipment.set(EquipmentSlot::Head, helmet.clone());
            mob.equipment.set_drop_chance(
                EquipmentSlot::Head,
                super::super::equipment::GUARANTEED_DROP_CHANCE,
            );
        }
        let tag = write_mob(&zombie.split()).expect("zombie saves");
        let Tag::Compound(entries) = &tag else {
            panic!("a compound");
        };
        let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
        let at = |key: &str| keys.iter().position(|candidate| *candidate == key).unwrap();
        assert!(at("equipment") > at("Brain"));
        assert!(at("equipment") < at("CanPickUpLoot"));
        assert_eq!(at("drop_chances"), at("PersistenceRequired") + 1);
        let row = read_mob(&tag, "minecraft:zombie", 3)
            .expect("reads")
            .expect("a zombie");
        let entity = row.into_entity();
        let mob = entity.kind.mob().expect("a mob");
        assert_eq!(mob.equipment.get(EquipmentSlot::Head), &helmet);
        assert!(mob.equipment.is_preserved(EquipmentSlot::Head));
    }

    /// 牛没有装备、几率是缺省值：两个键都不写。
    #[test]
    fn a_cow_writes_no_equipment_keys() {
        let cow = Entity::cow(Vec3::new(0.5, 64.0, 0.5));
        let tag = write_mob(&cow.split()).expect("cow saves");
        assert!(tag::field(&tag, "equipment").is_none());
        assert!(tag::field(&tag, "drop_chances").is_none());
    }
}
