//! `minecraft:egg` 的存档字段：`Projectile.addAdditionalSaveData`（`Owner`、`LeftOwner`、
//! `HasBeenShot`）加上 `ThrowableItemProjectile` 的 `Item`。
//!
//! The `minecraft:egg` save fields: `Projectile.addAdditionalSaveData` plus the
//! `Item` of `ThrowableItemProjectile`.

use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{components::archive::slots, item::ItemStack};

use super::{Error, Result, put, tag, uuid_tag};
use crate::entity::thrown_egg::{ThrowableKind, ThrownEgg};

pub(super) fn write(egg: &ThrownEgg, out: &mut Vec<(String, Tag)>) -> Result<()> {
    if egg.no_gravity {
        put(out, "NoGravity", Tag::Byte(1));
    }
    if let Some(uuid) = egg.owner_uuid {
        put(out, "Owner", uuid_tag(&uuid));
    }
    if egg.left_owner {
        put(out, "LeftOwner", Tag::Byte(1));
    }
    put(out, "HasBeenShot", Tag::Byte(i8::from(egg.has_been_shot)));
    let item = slots::encode_item(&egg.item).map_err(|error| Error::new(error.to_string()))?;
    put(out, "Item", item);
    Ok(())
}

/// 读不到 `Item` 时按 `getDefaultItem()` 给对应种类的默认物品。
///
/// Without `Item`, `getDefaultItem()` gives that kind's default item.
pub(super) fn read(input: &Tag, pitch: f32, kind: ThrowableKind) -> ThrownEgg {
    let item = tag::field(input, "Item")
        .and_then(|value| slots::decode_item(value).ok())
        .filter(|stack| !stack.is_empty())
        .unwrap_or_else(|| ItemStack::new(kind.default_item(), 1));
    let mut egg = ThrownEgg::with_kind(&item, kind);
    egg.x_rot = pitch;
    egg.uuid = super::arrow::uuid_from(input, "UUID").unwrap_or([0; 16]);
    egg.owner_uuid = super::arrow::uuid_from(input, "Owner");
    egg.no_gravity = tag::bool_or(input, "NoGravity", false);
    egg.left_owner = tag::bool_or(input, "LeftOwner", false);
    egg.has_been_shot = tag::bool_or(input, "HasBeenShot", false);
    egg
}
