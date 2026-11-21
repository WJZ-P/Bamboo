//! `Mob.convertTo` 的 `ConversionType.SINGLE`：把一头生物换成另一种，旧的丢弃。
//!
//! 公共机制，哞菇剪成牛用它；以后僵尸溺水转化、猪被雷劈也走这里。不建模的部分：
//! 乘客 / 载具、拴绳、睡觉位置、队伍、`Brain` 记忆（现有生物都没有）。
//!
//! `ConversionType.SINGLE` of `Mob.convertTo`: one mob replaced by another kind. Shared
//! by sheared mooshrooms; later conversions can reuse it. Passengers, vehicles, leashes,
//! sleeping positions, teams and `Brain` memories are not modelled (no current mob has
//! them).

use super::{living, spawn};
use crate::entity::Entity;

/// `ConversionParams.single(mob, keepEquipment, preserveCanPickUpLoot)` 的两个开关。
///
/// The two switches of `ConversionParams.single`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConversionParams {
    pub keep_equipment: bool,
    pub preserve_can_pick_up_loot: bool,
}

/// `ConversionType.SINGLE.convert`：`to` 已经构造好（ID、种子、UUID、朝向都取过），
/// 这里只抄状态。血量不抄，新生物是满血；`yHeadRot` 保留构造时抽的值。
///
/// `ConversionType.SINGLE.convert` onto an already constructed `to`. Health is not
/// copied, and `yHeadRot` keeps the constructor's draw.
pub(crate) fn convert_single(from: &Entity, to: &mut Entity, params: ConversionParams) {
    let Some(old) = from.kind.mob() else {
        return;
    };
    spawn::snap_to(to, from.position, from.y_rot, old.x_rot);
    to.delta = from.delta;
    to.fall_distance = from.fall_distance;
    to.on_ground = from.on_ground;
    to.with_mob(|me, mob| {
        if params.keep_equipment {
            for slot in super::equipment::SLOTS {
                let stack = old.equipment.get(slot);
                if !stack.is_empty() {
                    mob.equipment.set(slot, stack.clone());
                    mob.equipment
                        .set_drop_chance(slot, old.equipment.drop_chance(slot));
                }
            }
        }
        mob.fall_flying = old.fall_flying;
        mob.last_hurt_by_player_memory_time = old.last_hurt_by_player_memory_time;
        mob.hurt_time = old.hurt_time;
        mob.y_body_rot = old.y_body_rot;
        convert_common(old, me, mob, params);
    });
}

/// `ConversionType.convertCommon`。`setSharedFlagOnFire` 不单独抄：着火标志由
/// `remainingFireTicks` 推出来，官方下一刻 `baseTick` 也会按它重算。
///
/// `ConversionType.convertCommon`. The on-fire flag is not copied on its own: it follows
/// `remainingFireTicks`, as vanilla's next `baseTick` recomputes it.
fn convert_common(
    old: &super::Mob,
    me: &mut Entity,
    mob: &mut super::Mob,
    params: ConversionParams,
) {
    mob.absorption = old.absorption;
    for effect in old.effects.entries() {
        living::add_effect(mob, effect);
    }
    if !old.mob_type.is_monster() && !mob.mob_type.is_monster() {
        living::set_age(me, mob, old.age);
        mob.forced_age = old.forced_age;
        mob.forced_age_timer = old.forced_age_timer;
    }
    if params.preserve_can_pick_up_loot {
        mob.can_pick_up_loot = old.can_pick_up_loot;
    }
    mob.left_handed = old.left_handed;
    mob.no_ai = old.no_ai;
    if old.persistence_required {
        mob.persistence_required = true;
    }
    mob.custom_name_visible = old.custom_name_visible;
    mob.invulnerable = old.invulnerable;
    mob.invulnerable_time = old.invulnerable_time;
    mob.no_gravity = old.no_gravity;
    mob.portal_cooldown = old.portal_cooldown;
    mob.silent = old.silent;
    for tag in &old.tags {
        if !mob.tags.contains(tag) {
            mob.tags.push(tag.clone());
        }
    }
    // `COMPONENTS_TO_COPY`：`CUSTOM_NAME` 和 `CUSTOM_DATA`（存档里的 `data`）。
    // `COMPONENTS_TO_COPY`: `CUSTOM_NAME` and `CUSTOM_DATA` (the saved `data` key).
    if old.custom_name.is_some() {
        mob.custom_name = old.custom_name.clone();
    }
    if let Some(data) = old.passthrough.iter().find(|(key, _)| key == "data") {
        mob.passthrough.retain(|(key, _)| key != "data");
        mob.passthrough.push(data.clone());
    }
}
