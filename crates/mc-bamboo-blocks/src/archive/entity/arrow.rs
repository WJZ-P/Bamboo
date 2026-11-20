use mc_bamboo_data::sounds;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{components::archive::slots, item::ItemStack};

use super::{Error, Result, put, tag, uuid_tag};
use crate::entity::arrow::{Arrow, Pickup, arrow_hit_sound};

pub(super) fn write(arrow: &Arrow, out: &mut Vec<(String, Tag)>) -> Result<()> {
    if arrow.no_gravity {
        put(out, "NoGravity", Tag::Byte(1));
    }
    if let Some(uuid) = arrow.owner_uuid {
        put(out, "Owner", uuid_tag(&uuid));
    }
    if arrow.left_owner {
        put(out, "LeftOwner", Tag::Byte(1));
    }
    put(out, "HasBeenShot", Tag::Byte(i8::from(arrow.has_been_shot)));
    put(out, "life", Tag::Short(arrow.life as i16));
    if let Some(state) = arrow.last_state {
        put(out, "inBlockState", tag::encode_block_state(state));
    }
    put(out, "shake", Tag::Byte(arrow.shake_time as i8));
    put(out, "inGround", Tag::Byte(i8::from(arrow.in_ground)));
    let pickup = match arrow.pickup {
        Pickup::Disallowed => 0,
        Pickup::Allowed => 1,
        Pickup::CreativeOnly => 2,
    };
    put(out, "pickup", Tag::Byte(pickup));
    put(out, "damage", Tag::Double(arrow.base_damage));
    put(out, "crit", Tag::Byte(i8::from(arrow.critical)));
    put(out, "PierceLevel", Tag::Byte(arrow.pierce));
    if let Some(sound) = sounds::SOUND_EVENTS.get(usize::from(arrow.sound)) {
        put(out, "SoundEvent", Tag::String(sound.name.to_owned()));
    }
    put(out, "item", encode_item(&arrow.pickup_item)?);
    if let Some(weapon) = &arrow.weapon {
        put(out, "weapon", encode_item(weapon)?);
    }
    Ok(())
}

pub(super) fn read(input: &Tag, pitch: f32) -> Arrow {
    let mut arrow = Arrow::default();
    arrow.pitch = pitch;
    arrow.no_gravity = tag::bool_or(input, "NoGravity", false);
    arrow.uuid = uuid_from(input, "UUID").unwrap_or([0; 16]);
    arrow.owner_uuid = uuid_from(input, "Owner");
    arrow.left_owner = tag::bool_or(input, "LeftOwner", false);
    arrow.has_been_shot = tag::bool_or(input, "HasBeenShot", false);
    arrow.life = i32::from(tag::short_or(input, "life", 0));
    arrow.last_state =
        tag::field(input, "inBlockState").and_then(|t| tag::decode_block_state(t).ok());
    arrow.shake_time = tag::byte_or(input, "shake", 0) as u8;
    arrow.in_ground = tag::bool_or(input, "inGround", false);
    arrow.pickup = match tag::byte_or(input, "pickup", 0) {
        1 => Pickup::Allowed,
        2 => Pickup::CreativeOnly,
        _ => Pickup::Disallowed,
    };
    arrow.base_damage = tag::double_or(input, "damage", 2.0);
    arrow.critical = tag::bool_or(input, "crit", false);
    arrow.pierce = tag::byte_or(input, "PierceLevel", 0);
    arrow.sound = tag::string_or(input, "SoundEvent")
        .and_then(|name| {
            sounds::SOUND_EVENTS
                .iter()
                .position(|event| event.name == name)
        })
        .map(|id| id as u16)
        .unwrap_or_else(arrow_hit_sound);
    if let Some(item) = tag::field(input, "item").and_then(|t| slots::decode_item(t).ok())
        && !item.is_empty()
    {
        arrow.pickup_item = item;
    }
    arrow.weapon = tag::field(input, "weapon").and_then(|t| slots::decode_item(t).ok());
    arrow
}

fn uuid_from(input: &Tag, key: &str) -> Option<[u8; 16]> {
    let Tag::IntArray(words) = tag::field(input, key)? else {
        return None;
    };
    let [a, b, c, d] = words.as_slice() else {
        return None;
    };
    let mut bytes = [0; 16];
    for (output, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip([a, b, c, d]) {
        output.copy_from_slice(&word.to_be_bytes());
    }
    Some(bytes)
}

fn encode_item(stack: &ItemStack) -> Result<Tag> {
    slots::encode_item(stack).map_err(|error| Error::new(error.to_string()))
}
