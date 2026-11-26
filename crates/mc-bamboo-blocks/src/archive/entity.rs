//! 三种实体的官方存档 NBT，见 `docs/m3-storage.md` 5.5 与官方
//! `Entity.saveWithoutId` / `addAdditionalSaveData` / `readAdditionalSaveData`。
//!
//! 官方不存实体随机源、tick 计数、上一刻位置和碰撞标志；Bamboo 的这些字段读回时按
//! 构造器的默认值重建。`UUID` 由实体 ID 确定性派生，和网络层用同一个推导。
//! 不认识的实体 `id` 整条原样保留（第二段区块盒子里做）。

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{blocks, registries::entity_type as kinds};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{components::archive::slots, item::ItemStack};

mod arrow;
mod egg;
mod spit;
mod splash_potion;

use super::tag::{self, Error, Result, put};
use crate::{
    BlockState,
    entity::{
        Entity, EntityKind, EntityRow, EntityUid, ExperienceOrb, FallingBlock, GroundItem,
        JavaRandom, LeashKnot, PrimedTnt,
    },
};

/// 官方实体的 UUID 是随机的；Bamboo 由实体 ID 推出来，网络层和存档共用这一份。
pub const ENTITY_UUID_PREFIX: u64 = 0x0000_BAAB_0000_0000;

/// 和网络层的 `AddEntity.uuid` 完全一致的 128 位 UUID。
/// 高位 64 位是 `0x0000_BAAB_0000_0000`，低位 64 位是实体 ID。
pub fn entity_uuid(id: i32) -> u128 {
    (u128::from(ENTITY_UUID_PREFIX) << 64) | u128::from(id as u32 as u64)
}

/// 一行公开数据里的实体 UUID：`read_mob` 把存档的 `UUID` 读进 `MobPublic.uuid`，
/// 读回玩家档的 `RootVehicle/Attach` 时按它找到要重新骑上的那只载具。
///
/// The UUID in a row's public data: `read_mob` loads the saved `UUID` into
/// `MobPublic.uuid`, which `RootVehicle/Attach` matches on rejoin.
pub fn entity_uuid_of(row: &crate::entity::EntityRow) -> [u8; 16] {
    match &row.public.kind {
        crate::entity::KindPublic::Mob(mob) => mob.uuid,
        crate::entity::KindPublic::Arrow(arrow) => arrow.uuid,
        crate::entity::KindPublic::LlamaSpit(spit) => spit.uuid,
        crate::entity::KindPublic::Egg(egg) => egg.uuid,
        crate::entity::KindPublic::SplashPotion(potion) => potion.uuid,
        _ => entity_uuid(row.public.uid).to_be_bytes(),
    }
}

/// `UUIDUtil.CODEC` = `int[4]`（高位 32 位、低位 32 位、……）。
pub fn entity_uuid_tag(id: i32) -> Tag {
    let uuid = entity_uuid(id);
    // UUIDUtil.leASTMostToIntArray(most, least) 的实参是 (most >> 32, most, least >> 32, least)。
    let most = (uuid >> 64) as u64;
    let least = uuid as u64;
    Tag::IntArray(vec![
        (most >> 32) as i32,
        most as i32,
        (least >> 32) as i32,
        least as i32,
    ])
}

/// 官方 `Entity.saveWithoutId` 写出的共通键。`EntityStore` 里的 uid 当作网络实体 ID。
pub fn write_entity(row: &EntityRow) -> Result<Tag> {
    let mut out = Vec::new();
    let public = &row.public;
    let state = &row.state;
    let id = match &public.kind {
        crate::entity::KindPublic::FallingBlock { .. } => kinds::FALLING_BLOCK,
        crate::entity::KindPublic::Tnt { .. } => kinds::TNT,
        crate::entity::KindPublic::Item { .. } => kinds::ITEM,
        crate::entity::KindPublic::ExperienceOrb { .. } => kinds::EXPERIENCE_ORB,
        crate::entity::KindPublic::LeashKnot { .. } => kinds::LEASH_KNOT,
        crate::entity::KindPublic::Arrow(arrow) => arrow.variant.entity_type(),
        crate::entity::KindPublic::LlamaSpit(_) => kinds::LLAMA_SPIT,
        crate::entity::KindPublic::Egg(egg) => egg.kind.entity_type(),
        crate::entity::KindPublic::SplashPotion(_) => kinds::SPLASH_POTION,
        // 官方闪电实体不写进存档（`EntityType.canSerialize` 为假），这里不导出它。
        // Vanilla lightning bolts are not serialized; do not write them out.
        crate::entity::KindPublic::LightningBolt(_) => return Ok(Tag::Compound(Vec::new())),
        crate::entity::KindPublic::Mob(_) => return crate::entity::mob::nbt::write_mob(row),
    };
    put(
        &mut out,
        "id",
        Tag::String(
            kinds::name_of(id)
                .ok_or_else(|| Error::new("unknown entity type"))?
                .to_owned(),
        ),
    );
    put(&mut out, "Pos", vec3_tag(public.position));
    put(&mut out, "Motion", vec3_tag(public.delta));
    put(
        &mut out,
        "Rotation",
        Tag::List(vec![
            Tag::Float(public.yaw),
            Tag::Float(match &public.kind {
                crate::entity::KindPublic::Arrow(arrow) => arrow.pitch,
                crate::entity::KindPublic::LlamaSpit(spit) => spit.pitch,
                crate::entity::KindPublic::Egg(egg) => egg.x_rot,
                crate::entity::KindPublic::SplashPotion(potion) => potion.x_rot,
                _ => 0.0,
            }),
        ]),
    );
    put(&mut out, "fall_distance", Tag::Double(state.fall_distance));
    put(
        &mut out,
        "Fire",
        Tag::Short(clamp_short(state.remaining_fire_ticks)),
    );
    // 官方 getAirSupply() 默认 300；Bamboo 没有空气字段。
    put(&mut out, "Air", Tag::Short(300));
    put(&mut out, "OnGround", Tag::Byte(i8::from(public.on_ground)));
    put(&mut out, "Invulnerable", Tag::Byte(0));
    put(&mut out, "PortalCooldown", Tag::Int(0));
    let uuid = match &public.kind {
        crate::entity::KindPublic::Arrow(arrow) => uuid_tag(&arrow.uuid),
        crate::entity::KindPublic::LlamaSpit(spit) => uuid_tag(&spit.uuid),
        crate::entity::KindPublic::Egg(egg) => uuid_tag(&egg.uuid),
        crate::entity::KindPublic::SplashPotion(potion) => uuid_tag(&potion.uuid),
        _ => entity_uuid_tag(public.uid),
    };
    put(&mut out, "UUID", uuid);

    match &public.kind {
        crate::entity::KindPublic::FallingBlock { state: block, .. } => {
            let crate::entity::KindState::FallingBlock(saved) = &state.kind else {
                return Err(Error::new("mismatched falling block entity halves"));
            };
            put(&mut out, "BlockState", tag::encode_block_state(*block));
            put(&mut out, "Time", Tag::Int(saved.time));
            put(&mut out, "DropItem", Tag::Byte(i8::from(saved.drop_item)));
            put(
                &mut out,
                "HurtEntities",
                Tag::Byte(i8::from(saved.hurt_entities)),
            );
            put(
                &mut out,
                "FallHurtAmount",
                Tag::Float(saved.fall_damage_per_distance),
            );
            put(&mut out, "FallHurtMax", Tag::Int(saved.fall_damage_max));
            put(
                &mut out,
                "CancelDrop",
                Tag::Byte(i8::from(saved.cancel_drop)),
            );
        }
        crate::entity::KindPublic::Tnt { fuse } => {
            put(&mut out, "fuse", Tag::Short(clamp_short(*fuse)));
            put(
                &mut out,
                "block_state",
                tag::encode_block_state(BlockState(blocks::TNT)),
            );
        }
        crate::entity::KindPublic::Mob(_) | crate::entity::KindPublic::LightningBolt(_) => {}
        crate::entity::KindPublic::LeashKnot { pos } => {
            // `BlockAttachedEntity.addAdditionalSaveData`：`block_pos` 键。
            put(
                &mut out,
                "block_pos",
                Tag::IntArray(vec![pos.x, pos.y, pos.z]),
            );
        }
        crate::entity::KindPublic::Arrow(_) => {
            let crate::entity::KindState::Arrow(saved) = &state.kind else {
                return Err(Error::new("mismatched arrow entity halves"));
            };
            arrow::write(saved, &mut out)?;
        }
        crate::entity::KindPublic::LlamaSpit(_) => {
            let crate::entity::KindState::LlamaSpit(saved) = &state.kind else {
                return Err(Error::new("mismatched llama spit entity halves"));
            };
            spit::write(saved, &mut out);
        }
        crate::entity::KindPublic::Egg(_) => {
            let crate::entity::KindState::Egg(saved) = &state.kind else {
                return Err(Error::new("mismatched egg entity halves"));
            };
            egg::write(saved, &mut out)?;
        }
        crate::entity::KindPublic::SplashPotion(_) => {
            let crate::entity::KindState::SplashPotion(saved) = &state.kind else {
                return Err(Error::new("mismatched splash potion entity halves"));
            };
            splash_potion::write(saved, &mut out)?;
        }
        crate::entity::KindPublic::ExperienceOrb { value } => {
            let crate::entity::KindState::ExperienceOrb(saved) = &state.kind else {
                return Err(Error::new("mismatched experience orb entity halves"));
            };
            // 官方 `ExperienceOrb.addAdditionalSaveData` 的顺序：Health、Age、Value、Count。
            // Vanilla writes Health, Age, Value and Count in this order.
            put(&mut out, "Health", Tag::Short(clamp_short(saved.health)));
            put(&mut out, "Age", Tag::Short(clamp_short(saved.age)));
            put(&mut out, "Value", Tag::Short(clamp_short(*value)));
            put(&mut out, "Count", Tag::Int(saved.count));
        }
        crate::entity::KindPublic::Item {
            stack,
            pickup_delay,
            age,
            health,
            target,
        } => {
            put(&mut out, "Health", Tag::Short(clamp_short(*health)));
            put(&mut out, "Age", Tag::Short(clamp_short(*age)));
            put(
                &mut out,
                "PickupDelay",
                Tag::Short(clamp_short(*pickup_delay)),
            );
            if let Some(uuid) = target {
                put(&mut out, "Owner", uuid_tag(uuid));
            }
            if !stack.is_empty() {
                put(
                    &mut out,
                    "Item",
                    slots::encode_item(stack).map_err(|error| Error::new(error.to_string()))?,
                );
            }
        }
    }
    Ok(Tag::Compound(out))
}

/// `Passengers` 嵌套的最大深度。官方读档也递归，但盘上的坏数据不能把栈打穿。
///
/// Maximum nesting depth of `Passengers`. Vanilla recurses too, but corrupt
/// disk data must not overflow the stack.
const MAX_TREE_DEPTH: usize = 64;

/// 官方 `Entity.save`：写根实体和它的整棵乘客树。乘客不单独成行，而是按
/// `saveWithoutId` 末尾的 `Passengers` 嵌在载具里；乘客自己的 `Pos` 用载具的 x/z
/// 加自己的 y。`lookup` 按 uid 取同一份存档里的行，取不到或关系对不上就跳过
/// （和官方遍历真实对象列表等价，只是坏数据不 panic）。
///
/// Vanilla `Entity.save`: write the root and its whole passenger tree. Passengers
/// are not separate rows; they nest under the vehicle as `Passengers`, with their
/// `Pos` taken from the vehicle's x/z and their own y.
pub fn write_entity_tree(
    row: &EntityRow,
    lookup: &dyn Fn(EntityUid) -> Option<EntityRow>,
) -> Result<Tag> {
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(row.public.uid);
    write_tree(row, lookup, &mut seen, 0)
}

fn write_tree(
    row: &EntityRow,
    lookup: &dyn Fn(EntityUid) -> Option<EntityRow>,
    seen: &mut std::collections::BTreeSet<EntityUid>,
    depth: usize,
) -> Result<Tag> {
    let mut tag = write_entity(row)?;
    let crate::entity::KindPublic::Mob(mob) = &row.public.kind else {
        return Ok(tag);
    };
    if depth >= MAX_TREE_DEPTH {
        return Ok(tag);
    }
    let mut list = Vec::new();
    for &uid in &mob.passengers {
        if uid < crate::entity::FIRST_ENTITY_ID || !seen.insert(uid) {
            continue;
        }
        let Some(child) = lookup(uid) else {
            continue;
        };
        // 双向关系对不上时不信名单：坏数据里两边可能各说各话。
        if child.public.vehicle != Some(row.public.uid) {
            continue;
        }
        let mut child_tag = write_tree(&child, lookup, seen, depth + 1)?;
        set_pos_xz(&mut child_tag, row.public.position.x, row.public.position.z);
        list.push(child_tag);
    }
    if !list.is_empty()
        && let Tag::Compound(entries) = &mut tag
    {
        put(entries, "Passengers", Tag::List(list));
    }
    Ok(tag)
}

/// 把乘客的 `Pos` 改成载具的 x/z 加自己的 y（官方 `saveWithoutId` 的乘客分支）。
///
/// Rewrite a passenger's `Pos` to the vehicle's x/z plus its own y.
fn set_pos_xz(tag: &mut Tag, x: f64, z: f64) {
    let Tag::Compound(entries) = tag else {
        return;
    };
    for (key, value) in entries.iter_mut() {
        if key != "Pos" {
            continue;
        }
        if let Tag::List(values) = value
            && values.len() == 3
        {
            values[0] = Tag::Double(x);
            values[2] = Tag::Double(z);
        }
    }
}

/// 官方 `EntityType.loadEntityRecursive` 的 Bamboo 版：读根实体和嵌套的 `Passengers`。
/// 返回的行按深度优先排列，第一个是根；`next_uid` 从当前值起依次取号并向前推进。
/// 不认识的根 `id` 返回 [`Ok(None)`]，由调用方整条原样保留。
///
/// Vanilla `EntityType.loadEntityRecursive` in Bamboo: read the root and its nested
/// `Passengers`. Rows come depth-first with the root first; `next_uid` hands out
/// ids in turn and advances. An unknown root `id` returns [`Ok(None)`).
pub fn read_entity_tree(tag: &Tag, next_uid: &mut EntityUid) -> Result<Option<Vec<EntityRow>>> {
    read_tree(tag, next_uid, 0)
}

fn read_tree(tag: &Tag, next: &mut EntityUid, depth: usize) -> Result<Option<Vec<EntityRow>>> {
    let uid = *next;
    *next = uid
        .checked_add(1)
        .ok_or_else(|| Error::new("entity ids exhausted"))?;
    let Some(row) = read_entity(tag, uid)? else {
        return Ok(None);
    };
    let mut entity = row.into_entity();
    entity.id = uid;
    let mut out = Vec::new();
    if depth < MAX_TREE_DEPTH
        && let Some(Tag::List(children)) = tag::field(tag, "Passengers")
    {
        for child_tag in children {
            let Some(mut sub) = read_tree(child_tag, next, depth + 1)? else {
                continue;
            };
            let child_id = sub[0].public.uid;
            // `loadPassengersRecursive` 是 `startRiding(vehicle, true, false)`：
            // 两边的关系直接建立，不看冷却和乘客数。
            sub[0].public.vehicle = Some(uid);
            sub[0].state.boarding_cooldown = 0;
            entity.with_mob(|_, mob| {
                crate::entity::riding::add_passenger(&mut mob.passengers, child_id, false);
            });
            out.extend(sub);
        }
    }
    out.insert(0, entity.split());
    Ok(Some(out))
}

/// 官方 `EntityType.loadEntityRecursive` 的单实体版本（不含 `Passengers`）。
///
/// 不认识的 `id` 返回 [`Ok(None)`]，由调用方整条原样保留。`uid` 只是占位：放进岛的
/// 实体表时重新取号，随机源也在那时从岛的种子源取种子
/// （[`crate::entity::ChunkEntities::from_parts`]）。官方文件里的 `UUID` 不保留。
pub fn read_entity(tag: &Tag, uid: i32) -> Result<Option<EntityRow>> {
    let id = match tag::string_or(tag, "id") {
        Some(value) => value,
        None => return Ok(None),
    };
    let position = vec3_value(tag, "Pos", [0.0; 3]);
    let delta = vec3_value(tag, "Motion", [0.0; 3]);
    let rotation = tag::vec2(tag, "Rotation", [0.0; 2]);
    if let Some(row) = crate::entity::mob::nbt::read_mob(tag, id, uid)? {
        return Ok(Some(row));
    }
    let entity = Entity {
        id: uid,
        // `split` 不读尺寸，拼回来时按 `kind` 重算。
        dimensions: (0.0, 0.0),
        kind: match id {
            "minecraft:arrow" => EntityKind::Arrow(Box::new(arrow::read(
                tag,
                rotation[1],
                crate::entity::arrow::ArrowVariant::Normal,
            ))),
            "minecraft:spectral_arrow" => EntityKind::Arrow(Box::new(arrow::read(
                tag,
                rotation[1],
                crate::entity::arrow::ArrowVariant::Spectral { duration: 200 },
            ))),
            "minecraft:trident" => EntityKind::Arrow(Box::new(arrow::read(
                tag,
                rotation[1],
                crate::entity::arrow::ArrowVariant::Trident,
            ))),
            "minecraft:llama_spit" => EntityKind::LlamaSpit(Box::new(spit::read(tag, rotation[1]))),
            "minecraft:egg" => EntityKind::Egg(Box::new(egg::read(
                tag,
                rotation[1],
                crate::entity::thrown_egg::ThrowableKind::Egg,
            ))),
            "minecraft:snowball" => EntityKind::Egg(Box::new(egg::read(
                tag,
                rotation[1],
                crate::entity::thrown_egg::ThrowableKind::Snowball,
            ))),
            "minecraft:ender_pearl" => EntityKind::Egg(Box::new(egg::read(
                tag,
                rotation[1],
                crate::entity::thrown_egg::ThrowableKind::EnderPearl,
            ))),
            "minecraft:splash_potion" => {
                EntityKind::SplashPotion(Box::new(splash_potion::read(tag, rotation[1])))
            }
            "minecraft:falling_block" => {
                let block = match tag::field(tag, "BlockState") {
                    Some(value) => tag::decode_block_state(value)?,
                    None => BlockState(blocks::SAND),
                };
                let mut falling = FallingBlock::new(
                    block,
                    BlockPos::new(
                        position[0].floor() as i32,
                        position[1].floor() as i32,
                        position[2].floor() as i32,
                    ),
                );
                falling.time = tag::int_or(tag, "Time", 0);
                falling.drop_item = tag::bool_or(tag, "DropItem", true);
                falling.hurt_entities = tag::bool_or(tag, "HurtEntities", false);
                falling.fall_damage_per_distance = tag::float_or(tag, "FallHurtAmount", 0.0);
                falling.fall_damage_max = tag::int_or(tag, "FallHurtMax", 40);
                falling.cancel_drop = tag::bool_or(tag, "CancelDrop", false);
                EntityKind::FallingBlock(falling)
            }
            "minecraft:tnt" => EntityKind::Tnt(PrimedTnt {
                fuse: i32::from(tag::short_or(tag, "fuse", 80)),
                owner: None,
            }),
            "minecraft:item" => {
                let stack = match tag::field(tag, "Item") {
                    Some(value) => {
                        slots::decode_item(value).map_err(|error| Error::new(error.to_string()))?
                    }
                    None => ItemStack::empty(),
                };
                EntityKind::Item(GroundItem {
                    stack,
                    age: i32::from(tag::short_or(tag, "Age", 0)),
                    pickup_delay: i32::from(tag::short_or(tag, "PickupDelay", 0)),
                    health: i32::from(tag::short_or(tag, "Health", 5)),
                    bob_offs: 0.0,
                    roll_velocity: false,
                    target: match tag::field(tag, "Owner") {
                        Some(Tag::IntArray(values)) if values.len() == 4 => {
                            Some(uuid_bytes(values))
                        }
                        _ => None,
                    },
                })
            }
            "minecraft:lightning_bolt" => EntityKind::LightningBolt(Box::default()),
            "minecraft:leash_knot" => {
                let words = tag::int_array(tag, "block_pos");
                let pos = if words.len() == 3 {
                    BlockPos::new(words[0], words[1], words[2])
                } else {
                    BlockPos::new(0, 0, 0)
                };
                EntityKind::LeashKnot(LeashKnot { pos })
            }
            "minecraft:experience_orb" => {
                let value = i32::from(tag::short_or(tag, "Value", 0));
                let mut orb = ExperienceOrb::new(value);
                orb.health = i32::from(tag::short_or(tag, "Health", 5));
                orb.age = i32::from(tag::short_or(tag, "Age", 0));
                orb.count = tag::int_or(tag, "Count", 1).max(1);
                EntityKind::ExperienceOrb(orb)
            }
            _ => return Ok(None),
        },
        position: Vec3::new(position[0], position[1], position[2]),
        delta: Vec3::new(delta[0], delta[1], delta[2]),
        old_position: Vec3::new(position[0], position[1], position[2]),
        on_ground: tag::bool_or(tag, "OnGround", false),
        horizontal_collision: false,
        vertical_collision: false,
        vertical_collision_below: false,
        fall_distance: tag::double_or(tag, "fall_distance", 0.0),
        tick_count: 0,
        main_supporting_block: None,
        on_ground_no_blocks: false,
        in_water: false,
        removed: false,
        stuck_speed_multiplier: Vec3::default(),
        movement_this_tick: Vec::new(),
        final_movements: Vec::new(),
        requires_precise_position: false,
        // 占位，放进实体表时换成从岛的种子源取的种子。
        // Placeholder; replaced by a seed from the island's source on insert.
        random: JavaRandom::default(),
        y_rot: rotation[0],
        first_tick: true,
        remaining_fire_ticks: i32::from(tag::short_or(tag, "Fire", 0)),
        vehicle: None,
        boarding_cooldown: 0,
    };
    // 读盘不是新建实体：朝向按 `Rotation` 还原，不再取构造时的随机数，
    // `bob_offs` 这类官方不存的视觉量保持构造器默认值。
    Ok(Some(entity.split()))
}

fn vec3_tag(value: Vec3) -> Tag {
    Tag::List(vec![
        Tag::Double(value.x),
        Tag::Double(value.y),
        Tag::Double(value.z),
    ])
}

fn vec3_value(tag: &Tag, key: &str, default: [f64; 3]) -> [f64; 3] {
    tag::vec3(tag, key, default)
}

fn uuid_tag(bytes: &[u8; 16]) -> Tag {
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

fn uuid_bytes(words: &[i32]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (index, word) in words.iter().take(4).enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&(*word as u32).to_be_bytes());
    }
    out
}

fn clamp_short(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}
