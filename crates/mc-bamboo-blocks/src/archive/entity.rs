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

use super::tag::{self, Error, Result, put};
use crate::{
    BlockState,
    entity::{
        ENTITY_RANDOM_SEED, Entity, EntityKind, EntityRow, FallingBlock, GroundItem, JavaRandom,
        PrimedTnt,
    },
};

/// 官方实体的 UUID 是随机的；Bamboo 由实体 ID 推出来，网络层和存档共用这一份。
pub const ENTITY_UUID_PREFIX: u64 = 0x0000_BAAB_0000_0000;

/// 和网络层的 `AddEntity.uuid` 完全一致的 128 位 UUID。
/// 高位 64 位是 `0x0000_BAAB_0000_0000`，低位 64 位是实体 ID。
pub fn entity_uuid(id: i32) -> u128 {
    (u128::from(ENTITY_UUID_PREFIX) << 64) | u128::from(id as u32 as u64)
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
        Tag::List(vec![Tag::Float(public.yaw), Tag::Float(0.0)]),
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
    put(&mut out, "UUID", entity_uuid_tag(public.uid));

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

/// 官方 `EntityType.loadEntityRecursive` 的 Bamboo 版。
///
/// 不认识的 `id` 返回 [`Ok(None)`]，由调用方整条原样保留。`uid` 由生成流程重新分配，
/// 官方文件里的 `UUID` 不保留；实体随机源按 uid 重建。
pub fn read_entity(tag: &Tag, uid: i32) -> Result<Option<EntityRow>> {
    let id = match tag::string_or(tag, "id") {
        Some(value) => value,
        None => return Ok(None),
    };
    let position = vec3_value(tag, "Pos", [0.0; 3]);
    let delta = vec3_value(tag, "Motion", [0.0; 3]);
    let rotation = tag::vec2(tag, "Rotation", [0.0; 2]);
    let entity = Entity {
        id: uid,
        kind: match id {
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
        random: JavaRandom::new(i64::from(uid) ^ ENTITY_RANDOM_SEED),
        y_rot: rotation[0],
        first_tick: true,
        remaining_fire_ticks: i32::from(tag::short_or(tag, "Fire", 0)),
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
