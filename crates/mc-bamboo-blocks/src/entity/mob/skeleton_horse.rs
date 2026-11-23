//! 骷髅马和僵尸马：官方 `SkeletonHorse` / `ZombieHorse`。
//!
//! 两者都继承 `AbstractHorse`，属性和存档的小块差异在这里；行为和马科共用 `equine`。
//! 骷髅陷阱马自带 `SkeletonTrapGoal`：玩家靠近时再劈一道视觉闪电，召来四名骷髅骑士。
//!
//! Skeleton and zombie horses: vanilla `SkeletonHorse` / `ZombieHorse`. Both
//! extend `AbstractHorse`; only their attributes, dimensions, save keys and the
//! skeleton trap differ from the common equine behaviour.

use mc_bamboo_data::{entity_types, registries::attribute};

use super::{Mob, MobType, attributes::Attributes};
use crate::{entity::Entity, random::RandomSource};

/// `SkeletonHorse.TRAP_MAX_LIFE`，没被打断的陷阱马 18000 刻后消失。
pub const TRAP_MAX_LIFE: i32 = 18_000;
/// `SkeletonTrapGoal.canUse` 的玩家距离。
pub const TRAP_PLAYER_DISTANCE: f64 = 10.0;

/// 骷髅马自己的状态：`isTrap` 和 `trapTime`，以及存档里的两个键。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkeletonHorseState {
    /// `SkeletonTrap`，`setTrap` 时挂上 / 摘掉陷阱目标。
    pub trap: bool,
    /// `SkeletonTrapTime`，`aiStep` 里累加。
    pub trap_time: i32,
}

/// `SkeletonHorse.createAttributes`：`createBaseHorseAttributes` 加血量和速度。
pub(super) fn skeleton_attributes() -> Attributes {
    let mut attrs = super::equine::attributes(MobType::SkeletonHorse);
    attrs.set_base(
        attribute::MAX_HEALTH,
        entity_types::SKELETON_HORSE_MAX_HEALTH,
    );
    attrs.set_base(
        attribute::MOVEMENT_SPEED,
        entity_types::SKELETON_HORSE_MOVEMENT_SPEED,
    );
    attrs
}

/// `ZombieHorse.createAttributes`：`createBaseHorseAttributes` 加血量。
pub(super) fn zombie_attributes() -> Attributes {
    let mut attrs = super::equine::attributes(MobType::ZombieHorse);
    attrs.set_base(
        attribute::MAX_HEALTH,
        entity_types::ZOMBIE_HORSE_MAX_HEALTH,
    );
    attrs
}

/// `SkeletonHorse.randomizeAttributes`：只抽跳跃力，用基类的三数求和公式。
pub(super) fn randomize_skeleton(mob: &mut Mob, random: &dyn RandomSource) {
    let jump = super::equine::horse::jump(|| random.next_double());
    if let Some(entry) = mob.attributes.instance(attribute::JUMP_STRENGTH) {
        entry.base = jump;
    }
}

/// `ZombieHorse.generateZombieHorseJumpStrength`：`0.5 + (d1+d2+d3)/15`。
pub(super) fn randomize_zombie_jump(random: &dyn RandomSource) -> f64 {
    0.5 + (random.next_double() + random.next_double() + random.next_double()) * (1.0 / 15.0)
}

/// `ZombieHorse.generateZombieHorseSpeed`：`(9 + d1 + d2 + d3) / 42.16`。
pub(super) fn randomize_zombie_speed(random: &dyn RandomSource) -> f64 {
    (9.0 + random.next_double() + random.next_double() + random.next_double())
        / f64::from(42.16_f32)
}

/// `SkeletonHorse.aiStep`：持久的陷阱马不数时间；数到 18000 就消失。
pub(super) fn tick_trap(me: &mut Entity, mob: &mut Mob) -> bool {
    if mob.persistence_required {
        return false;
    }
    let Some(state) = mob.skeleton_horse.as_mut() else {
        return false;
    };
    if !state.trap {
        return false;
    }
    state.trap_time += 1;
    if state.trap_time >= TRAP_MAX_LIFE {
        me.discard();
        return true;
    }
    false
}
