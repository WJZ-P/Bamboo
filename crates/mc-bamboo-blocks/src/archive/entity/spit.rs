//! 羊驼口水的存档：`Projectile.addAdditionalSaveData` / `readAdditionalSaveData`。
//! `LlamaSpit` 自己不加键。
//!
//! Llama spit saves: `Projectile.addAdditionalSaveData` and
//! `readAdditionalSaveData`; `LlamaSpit` adds no keys of its own.

use mc_bamboo_nbt::Tag;

use super::{arrow::uuid_from, put, tag, uuid_tag};
use crate::entity::spit::LlamaSpit;

pub(super) fn write(spit: &LlamaSpit, out: &mut Vec<(String, Tag)>) {
    if let Some(uuid) = spit.owner_uuid {
        put(out, "Owner", uuid_tag(&uuid));
    }
    if spit.left_owner {
        put(out, "LeftOwner", Tag::Byte(1));
    }
    put(out, "HasBeenShot", Tag::Byte(i8::from(spit.has_been_shot)));
}

pub(super) fn read(input: &Tag, pitch: f32) -> LlamaSpit {
    LlamaSpit {
        uuid: uuid_from(input, "UUID").unwrap_or([0; 16]),
        owner_uuid: uuid_from(input, "Owner"),
        pitch,
        left_owner: tag::bool_or(input, "LeftOwner", false),
        has_been_shot: tag::bool_or(input, "HasBeenShot", false),
        ..LlamaSpit::default()
    }
}
