//! 被闪电劈中的生物，官方 `Entity.thunderHit` 的覆盖层。
//!
//! 哞菇、苦力怕、海龟各自覆写了它，其余走基类（着火 + 5 点雷击伤害）。村民和猪的
//! 转换（女巫 / 僵尸猪灵）不在这一批的建模范围，README 记为已知限制。

use super::mooshroom;
use crate::world::World;

/// `Creeper.thunderHit` 的第二半：走完父类伤害之后 `DATA_IS_POWERED` 置真。
pub fn set_creeper_powered(world: &mut World, uid: i32) {
    world.with_entity_mut(uid, &mut |entity| {
        entity.with_mob(|_, mob| {
            mob.creeper.powered = true;
            mob.needs_sync = true;
        });
    });
}

/// `MushroomCow.thunderHit`：同一道闪电只换一次色。
pub fn mooshroom_hit(world: &mut World, uid: i32, bolt: [u8; 16]) {
    world.with_entity_mut(uid, &mut |entity| {
        entity.with_mob(|me, mob| mooshroom::thunder_hit(me, mob, bolt));
    });
}
