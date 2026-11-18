//! 生物的存档 NBT：官方 `Entity.saveWithoutId` / `load` 加上 `LivingEntity`、`Mob`、
//! `AgeableMob`、`Animal`、`Cow` 各层的 `addAdditionalSaveData` / `readAdditionalSaveData`。
//!
//! 读写的键、类型、缺省值和「只在非默认时写」的条件逐条照官方字节码。官方有、这里
//! 不建模的键（效果、装备、拴绳、`data`、睡觉位置……）读进来放进
//! [`Mob::passthrough`]，存盘时原样写回；不认识的键和官方一样丢掉。`/summon` 的 NBT
//! 也走这里（官方 `SummonCommand` 用 `EntityType.loadEntityRecursive`）。
//!
//! Mob archive NBT, following vanilla `Entity.saveWithoutId` / `load` and each
//! layer's additional save data. Vanilla keys that are not modelled go into
//! [`Mob::passthrough`] and are written back verbatim; unknown keys are dropped
//! like vanilla does. `/summon` NBT goes through here too.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::registries::attribute;
use mc_bamboo_nbt::Tag;

use super::{
    CowSoundVariant, CowVariant, Mob,
    attributes::{Attribute, Modifier, Operation},
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
    "active_effects",
    "equipment",
    "drop_chances",
    "leash",
    "DeathLootTable",
    "data",
    "HasVisualFire",
    "locator_bar_icon",
    "sleeping_pos",
    "current_explosion_impact_pos",
];

/// `Brain.Packed.CODEC` 写一头牛的空大脑。
fn empty_brain() -> Tag {
    Tag::Compound(vec![("memories".to_owned(), Tag::Compound(Vec::new()))])
}

/// 写一个生物：官方 `Entity.save` 的结果（含 `id`）。
pub fn write_mob(row: &EntityRow) -> Result<Tag> {
    let public = &row.public;
    let state = &row.state;
    let (KindPublic::Mob(_), KindState::Mob(mob)) = (&public.kind, &state.kind) else {
        return Err(Error::new("mismatched mob entity halves"));
    };
    let mut out = Vec::new();
    put(&mut out, "id", Tag::String("minecraft:cow".to_owned()));
    // ---- Entity.saveWithoutId ----
    put(&mut out, "Pos", vec3_tag(public.position));
    put(&mut out, "Motion", vec3_tag(public.delta));
    put(
        &mut out,
        "Rotation",
        Tag::List(vec![Tag::Float(public.yaw), Tag::Float(mob.x_rot)]),
    );
    put(&mut out, "fall_distance", Tag::Double(state.fall_distance));
    put(&mut out, "Fire", Tag::Short(state.remaining_fire_ticks as i16));
    put(&mut out, "Air", Tag::Short(mob.air_supply as i16));
    put(&mut out, "OnGround", Tag::Byte(i8::from(public.on_ground)));
    put(&mut out, "Invulnerable", Tag::Byte(i8::from(mob.invulnerable)));
    put(&mut out, "PortalCooldown", Tag::Int(mob.portal_cooldown));
    if mob.invulnerable_time > 0 {
        put(&mut out, "invulnerable_time", Tag::Int(mob.invulnerable_time));
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
    put(&mut out, "FallFlying", Tag::Byte(i8::from(mob.fall_flying)));
    put(&mut out, "Brain", empty_brain());
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
    if let Some((pos, radius)) = mob.home {
        put(&mut out, "home_radius", Tag::Int(radius));
        put(&mut out, "home_pos", block_pos_tag(pos));
    }
    put(&mut out, "LeftHanded", Tag::Byte(i8::from(mob.left_handed)));
    if mob.loot_table_seed != 0 {
        put(&mut out, "DeathLootTableSeed", Tag::Long(mob.loot_table_seed));
    }
    if mob.no_ai {
        put(&mut out, "NoAI", Tag::Byte(1));
    }
    // ---- AgeableMob / Animal ----
    put(&mut out, "Age", Tag::Int(mob.age));
    put(&mut out, "ForcedAge", Tag::Int(mob.forced_age));
    put(&mut out, "AgeLocked", Tag::Byte(i8::from(mob.age_locked)));
    put(&mut out, "InLove", Tag::Int(mob.in_love));
    if let Some(uuid) = &mob.love_cause {
        put(&mut out, "LoveCause", uuid_tag(uuid));
    }
    // ---- Cow ----
    put(&mut out, "variant", Tag::String(mob.variant.name().to_owned()));
    put(
        &mut out,
        "sound_variant",
        Tag::String(mob.sound_variant.name().to_owned()),
    );
    for (key, value) in &mob.passthrough {
        out.retain(|(existing, _)| existing != key);
        out.push((key.clone(), value.clone()));
    }
    Ok(Tag::Compound(out))
}

/// 读一个生物；不是这一轮支持的生物时返回 `None`，交回调用方照旧处理。
pub fn read_mob(tag: &Tag, id: &str, uid: i32) -> Result<Option<EntityRow>> {
    if id != "minecraft:cow" {
        return Ok(None);
    }
    let mut entity = Entity::cow(Vec3::default());
    entity.id = uid;
    // 读盘的实体不是新建的：构造时的随机数（UUID、朝向）不取，缺 `UUID` 时全零。
    // A loaded entity is not freshly built: the constructor draws are skipped.
    load_into(&mut entity, tag)?;
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
        mob.y_head_rot = me.y_rot;
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
        // ---- LivingEntity.readAdditionalSaveData ----
        mob.absorption = tag::float_or(tag, "AbsorptionAmount", 0.0).max(0.0);
        if let Some(Tag::List(entries)) = tag::field(tag, "attributes") {
            apply_attributes(mob, entries);
        }
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
        mob.last_hurt_by_mob_timestamp = me
            .tick_count
            .wrapping_sub(tag::int_or(tag, "ticks_since_last_hurt_by_mob", 0));
        mob.impulse_grace_time = tag::int_or(tag, "current_impulse_context_reset_grace_time", 0);
        // ---- Mob.readAdditionalSaveData ----
        mob.can_pick_up_loot = tag::bool_or(tag, "CanPickUpLoot", false);
        mob.persistence_required = tag::bool_or(tag, "PersistenceRequired", false);
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
        mob.left_handed = tag::bool_or(tag, "LeftHanded", false);
        mob.loot_table_seed = tag::long_or(tag, "DeathLootTableSeed", 0);
        mob.no_ai = tag::bool_or(tag, "NoAI", false);
        // ---- AgeableMob / Animal ----
        living::set_age(me, mob, tag::int_or(tag, "Age", 0));
        mob.forced_age = tag::int_or(tag, "ForcedAge", 0);
        mob.age_locked = tag::bool_or(tag, "AgeLocked", false);
        mob.in_love = tag::int_or(tag, "InLove", 0);
        mob.love_cause = uuid_field(tag, "LoveCause");
        // ---- Cow ----
        if let Some(variant) = tag::string_or(tag, "variant").and_then(CowVariant::from_name) {
            mob.variant = variant;
        }
        if let Some(variant) =
            tag::string_or(tag, "sound_variant").and_then(CowSoundVariant::from_name)
        {
            mob.sound_variant = variant;
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
        if let Some(base) = tag::field(entry, "base").and_then(|value| tag::java_f64(value).ok())
        {
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

fn uuid_field(tag: &Tag, key: &str) -> Option<[u8; 16]> {
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
