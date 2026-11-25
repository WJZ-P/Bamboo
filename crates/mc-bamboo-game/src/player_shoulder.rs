//! 玩家肩膀上的鹦鹉：上肩、离肩条件、把鹦鹉放回世界。
//!
//! Parrots perched on a player's shoulders: perching, removal conditions and
//! respawning the parrot back into the world.

use mc_bamboo_core::Vec3;
use mc_bamboo_nbt::Tag;

use crate::{player::Player, region::Region};

/// `ShoulderEntity*` 的 `Variant`（旧版整数编码）；空肩 / 坏值返回 `None`。
///
/// The `Variant` of a shoulder entity (legacy integer coding); `None` for an
/// empty shoulder or a malformed tag.
pub fn variant_of(tag: &Option<Tag>) -> Option<i32> {
    let Tag::Compound(entries) = tag.as_ref()? else {
        return None;
    };
    entries
        .iter()
        .find_map(|(key, value)| match (key.as_str(), value) {
            ("Variant", Tag::Int(variant)) => Some(*variant),
            _ => None,
        })
}

/// `ServerPlayer.setEntityOnShoulder`：左肩空先左，右肩空再右，都满返回 false。
/// 成功时把相对倒计时归零（官方上肩后 20 刻内不因离肩条件落下）。
///
/// `ServerPlayer.setEntityOnShoulder`: left first, then right, false when both
/// are occupied. A successful perch resets the relative 20-tick removal lock.
pub fn set_entity_on_shoulder(player: &mut Player, tag: Tag) -> bool {
    let slot = if player.shoulder_left.is_none() {
        &mut player.shoulder_left
    } else if player.shoulder_right.is_none() {
        &mut player.shoulder_right
    } else {
        return false;
    };
    *slot = Some(tag);
    player.shoulder_since = 0;
    true
}

/// `ServerPlayer.handleShoulderEntities`：每刻推进相对倒计时；上肩 20 刻后遇到离肩
/// 判据就把两只鹦鹉放回世界（官方 `timeEntitySatOnShoulder + 20 < gameTime`）。
///
/// `ServerPlayer.handleShoulderEntities`: advance the relative counter and, after
/// 20 ticks, respawn both parrots when a removal condition holds.
pub fn tick(region: &mut Region) {
    let mut respawns: Vec<(Vec3, Tag)> = Vec::new();
    for player in region.players.values_mut() {
        player.shoulder_since = player.shoulder_since.saturating_add(1);
        if player.shoulder_left.is_none() && player.shoulder_right.is_none() {
            continue;
        }
        if player.shoulder_since > 20 && should_remove(player) {
            let pos = player.position();
            if let Some(tag) = player.shoulder_left.take() {
                respawns.push((pos, tag));
            }
            if let Some(tag) = player.shoulder_right.take() {
                respawns.push((pos, tag));
            }
            player.shoulder_since = i32::MAX;
        }
    }
    for (pos, tag) in respawns {
        respawn(region, pos, &tag);
    }
}

/// `ServerPlayer.handleShoulderEntities` 的离肩判据：摔落距离、在水里 / 游泳、飞行、
/// 睡觉、细雪。
fn should_remove(player: &Player) -> bool {
    player.motion.fall_distance > 0.5
        || player.motion.swimming
        || player.sync.flying
        || player.sleep.is_some()
        || player.environment.in_powder_snow
}

/// `ServerPlayer.respawnEntityOnShoulder`：`EntityType.create(LOAD)` 从 NBT 造回鹦鹉，
/// 放到玩家位置。坏 NBT 直接跳过，不 panic。
fn respawn(region: &mut Region, position: Vec3, tag: &Tag) {
    if let Ok(Some(row)) = mc_bamboo_blocks::entity::mob::nbt::read_mob(tag, "minecraft:parrot", 0)
    {
        let mut entity = row.into_entity();
        entity.position = position;
        region.entities.spawn(entity);
    }
}
