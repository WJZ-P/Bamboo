//! 铁傀儡的村庄目标：官方 `MoveTowardsTargetGoal` / `MoveBackToVillageGoal` /
//! `GolemRandomStrollInVillageGoal` / `OfferFlowerGoal` / `DefendVillageTargetGoal`。
//!
//! Bamboo 没有村民和村庄 POI，这里实现的是「周围没有村庄时」官方真正走的那条路径：
//! 朝目标用 `DefaultRandomPos.getPosTowards` 找落点、闲逛退回 `LandRandomPos.getPos`、
//! 回村找村庄段找不到就放弃。取数次数、顺序和随机数来源都照字节码。
//!
//! Iron-golem village goals, implemented along vanilla's no-villager/no-POI path.

use mc_bamboo_core::Vec3;

use super::{
    Mob,
    goals::{combat_target, land_random_pos, pos_towards, reduced_tick_delay},
    living,
};
use crate::entity::{Entity, EntityUid, ctx::EntityCtx};

/// `DefaultRandomPos.getPosTowards` 的扇形半角，官方是编译期折好的 double 常量。
const TOWARDS_RADIANS: f64 = 1.570_796_370_506_286_6;

/// `MoveTowardsTargetGoal.canUse`：目标在 `within` 格内时，朝它抽 10 次落点。
pub(super) fn move_towards_can_use(
    me: &mut Entity,
    mob: &Mob,
    ctx: &mut dyn EntityCtx,
    within: f32,
    wanted: &mut Vec3,
    target: &mut Option<EntityUid>,
) -> bool {
    let Some(found) = combat_target(mob, ctx) else {
        return false;
    };
    if found.position.distance_to_sqr(me.position) > f64::from(within * within) {
        return false;
    }
    let Some(pos) = pos_towards(
        me,
        mob,
        ctx.blocks(),
        16,
        7,
        found.position,
        TOWARDS_RADIANS,
    ) else {
        return false;
    };
    *wanted = pos;
    *target = Some(found.uid);
    true
}

/// `MoveTowardsTargetGoal.canContinueToUse`。
pub(super) fn move_towards_can_continue(
    me: &Entity,
    mob: &Mob,
    ctx: &dyn EntityCtx,
    within: f32,
    target: Option<EntityUid>,
) -> bool {
    if mob.navigation.is_done() {
        return false;
    }
    let Some(uid) = target else {
        return false;
    };
    let Some(found) = combat_target(mob, ctx) else {
        return false;
    };
    found.uid == uid
        && found.health > 0.0
        && found.position.distance_to_sqr(me.position) < f64::from(within * within)
}

/// `RandomStrollGoal.canUse` 加 `getPosition()`。
///
/// `back` 为真时是 `MoveBackToVillageGoal`：没有村庄段，抽完 `nextInt(5)` 后
/// `findSectionClosestToVillage` 找不到别的段，`getPosition` 返回 null。
/// 否则是 `GolemRandomStrollInVillageGoal`：先用世界随机源抽一到两个 `nextFloat`
/// 决定走哪一支，没有村民 / POI 时退回 `LandRandomPos.getPos(10, 7)`。
pub(super) fn stroll_can_use(
    me: &mut Entity,
    mob: &Mob,
    ctx: &mut dyn EntityCtx,
    interval: i32,
    wanted: &mut Vec3,
    force_trigger: &mut bool,
    back: bool,
) -> bool {
    if !*force_trigger && me.random.next_int_bounded(reduced_tick_delay(interval)) != 0 {
        return false;
    }
    if back {
        // `MoveBackToVillageGoal.getPosition`：没有村庄段时恒为 null。
        return false;
    }
    let first = ctx.blocks().level_random_float();
    if first < 0.3 {
        // 直接 `getPositionTowardsAnywhere()`，不再取第二个世界随机数。
    } else {
        let second = ctx.blocks().level_random_float();
        let _ = second;
        // 两个候选（想要傀儡的村民、村庄 POI）都找不到，最后仍退回普通随机落点。
    }
    let Some(found) = land_random_pos(me, mob, ctx.blocks(), 10, 7) else {
        return false;
    };
    *wanted = found;
    *force_trigger = false;
    true
}

pub(super) fn stroll_start(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    wanted: Vec3,
    speed: f64,
) {
    let nav = living::nav_mob(me, mob, ctx.blocks());
    mob.navigation.move_to(ctx.blocks(), &nav, wanted, speed);
}

/// `OfferFlowerGoal.canUse`：白天先抽 `nextInt(8000)`，没有村民时找不到送礼对象。
pub(super) fn offer_flower_can_use(me: &mut Entity, ctx: &dyn EntityCtx) -> bool {
    if !ctx.blocks().is_bright_outside() {
        return false;
    }
    if me.random.next_int_bounded(8000) != 0 {
        return false;
    }
    // `ServerLevel.getNearestEntity(CANDIDATE_FOR_IRON_GOLEM_GIFT, ...)`：夹具里没有村民。
    false
}

/// `DefendVillageTargetGoal.canUse`：附近没有村民，`getNearbyEntities` 返回空表，
/// 不取随机数，直接返回 false。
pub(super) const fn defend_village_can_use() -> bool {
    false
}

/// `OfferFlowerGoal.tick`。
pub(super) fn offer_flower_tick(tick: &mut i32) {
    *tick -= 1;
}

/// `MoveTowardsTargetGoal.stop`。
pub(super) fn clear_target(target: &mut Option<EntityUid>) {
    *target = None;
}
