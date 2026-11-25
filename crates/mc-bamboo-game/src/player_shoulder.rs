//! 玩家肩膀上的鹦鹉：上肩、离肩条件、把鹦鹉放回世界。
//!
//! Parrots perched on a player's shoulders: perching, removal conditions and
//! respawning the parrot back into the world.

use mc_bamboo_nbt::Tag;

use crate::player::Player;

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
