//! `minecraft:splash_potion` 的存档字段：和鸡蛋一样是 `Projectile.addAdditionalSaveData`
//! （`Owner`、`LeftOwner`、`HasBeenShot`）加 `ThrowableItemProjectile` 的 `Item`。
//!
//! The `minecraft:splash_potion` save fields: the same `Projectile.addAdditionalSaveData`
//! keys as an egg plus `ThrowableItemProjectile`'s `Item`.

use mc_bamboo_data::items;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{components::archive::slots, item::ItemStack};

use super::{Error, Result, put, tag, uuid_tag};
use crate::entity::splash_potion::SplashPotion;

pub(super) fn write(potion: &SplashPotion, out: &mut Vec<(String, Tag)>) -> Result<()> {
    if potion.no_gravity {
        put(out, "NoGravity", Tag::Byte(1));
    }
    if let Some(uuid) = potion.owner_uuid {
        put(out, "Owner", uuid_tag(&uuid));
    }
    if potion.left_owner {
        put(out, "LeftOwner", Tag::Byte(1));
    }
    put(
        out,
        "HasBeenShot",
        Tag::Byte(i8::from(potion.has_been_shot)),
    );
    let item = slots::encode_item(&potion.item).map_err(|error| Error::new(error.to_string()))?;
    put(out, "Item", item);
    Ok(())
}

/// 读不到 `Item` 时按 `getDefaultItem()` 给一瓶普通喷溅药水。
///
/// Without `Item`, `getDefaultItem()` gives one plain splash potion.
pub(super) fn read(input: &Tag, pitch: f32) -> SplashPotion {
    let item = tag::field(input, "Item")
        .and_then(|value| slots::decode_item(value).ok())
        .filter(|stack| !stack.is_empty())
        .unwrap_or_else(|| ItemStack::new(items::SPLASH_POTION, 1));
    let mut potion = SplashPotion::new(&item);
    potion.x_rot = pitch;
    potion.uuid = super::arrow::uuid_from(input, "UUID").unwrap_or([0; 16]);
    potion.owner_uuid = super::arrow::uuid_from(input, "Owner");
    potion.no_gravity = tag::bool_or(input, "NoGravity", false);
    potion.left_owner = tag::bool_or(input, "LeftOwner", false);
    potion.has_been_shot = tag::bool_or(input, "HasBeenShot", false);
    potion
}
