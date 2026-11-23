//! 骷髅马和僵尸马：官方 `SkeletonHorse` / `ZombieHorse`。
//!
//! 两者都继承 `AbstractHorse`，属性和存档的小块差异在这里；行为和马科共用 `equine`。
//! 骷髅陷阱马自带 `SkeletonTrapGoal`：玩家靠近时再劈一道视觉闪电，召来四名骷髅骑士。
//!
//! Skeleton and zombie horses: vanilla `SkeletonHorse` / `ZombieHorse`. Both
//! extend `AbstractHorse`; only their attributes, dimensions, save keys and the
//! skeleton trap differ from the common equine behaviour.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{entity_types, registries::attribute};

use super::{Mob, MobType, attributes::Attributes};
use crate::{
    entity::{Entity, ctx::BlockView, ctx::EntityCtx},
    random::RandomSource,
    world::World,
};
use super::goals::{Goal, GoalSelector, WrappedGoal};

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

/// `SkeletonTrapGoal.canUse`：`ServerLevel.hasNearbyAlivePlayer(x, y, z, 10)`。
pub fn trap_player_near(position: Vec3, blocks: &dyn BlockView) -> bool {
    let mut found = false;
    blocks.for_each_player(&mut |player| {
        if !found && player.alive && !player.spectator {
            let dx = player.position.x - position.x;
            let dy = player.position.y - position.y;
            let dz = player.position.z - position.z;
            found = dx * dx + dy * dy + dz * dz < TRAP_PLAYER_DISTANCE * TRAP_PLAYER_DISTANCE;
        }
    });
    found
}

/// `SkeletonTrapGoal.tick` 的算阶段：清陷阱标记、驯服、成年，然后把生成整批实体
/// 交给落地命令（骷髅骑士和视觉闪电都要写实体表）。
pub fn trap_tick(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
) -> TrapSpawn {
    let _ = ctx;
    if let Some(state) = mob.skeleton_horse.as_mut() {
        state.trap = false;
    }
    if let Some(state) = mob.equine.as_mut() {
        state.tame = true;
    }
    mob.age = 0;
    // 三匹新马的水平推力在官方用原陷阱马的实体随机源取两个 `triangle(0, 1.1485)`。
    let mut random = me.random;
    let pushes: Vec<(f64, f64)> = (0..3)
        .map(|_| {
            (
                random.triangle(0.0, 1.1485),
                random.triangle(0.0, 1.1485),
            )
        })
        .collect();
    me.random = random;
    TrapSpawn {
        position: me.position,
        pushes,
        effective: ctx.blocks().effective_difficulty(me.block_position()),
    }
}

/// 一次陷阱触发的落地事实。
#[derive(Debug, Clone, PartialEq)]
pub struct TrapSpawn {
    pub position: Vec3,
    /// 三匹新骷髅马分别被推的 (dx, dz)。
    pub pushes: Vec<(f64, f64)>,
    pub effective: f32,
}

/// `SkeletonTrapGoal.tick` 的落地：一道视觉闪电、原马上的骷髅和三匹新的骷髅马骑士。
pub fn spawn_trap(world: &mut World, facts: TrapSpawn) {
    let bolt = crate::entity::lightning::entity(facts.position, true);
    world.spawn_entity(bolt);
    for (index, (dx, dz)) in facts.pushes.iter().enumerate() {
        let _ = index;
        let horse_position = facts.position;
        let mut horse = Entity::by_mob_type(MobType::SkeletonHorse, horse_position);
        horse.with_mob(|_, mob| {
            if let Some(state) = mob.equine.as_mut() {
                state.tame = true;
            }
            mob.age = 0;
            mob.invulnerable_time = 60;
            mob.persistence_required = true;
        });
        horse.delta = Vec3::new(*dx, 0.0, *dz);
        world.spawn_entity(horse);
        let mut rider = Entity::by_mob_type(MobType::Skeleton, horse_position);
        rider.with_mob(|_, mob| {
            mob.invulnerable_time = 60;
            mob.persistence_required = true;
            mob.equipment.set(
                mc_bamboo_protocol::packet::play::EquipmentSlot::Head,
                mc_bamboo_protocol::item::ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1),
            );
        });
        world.spawn_entity(rider);
    }
    // 骑在原陷阱马上的第一名骷髅。
    let mut rider = Entity::by_mob_type(MobType::Skeleton, facts.position);
    rider.with_mob(|_, mob| {
        mob.invulnerable_time = 60;
        mob.persistence_required = true;
        mob.equipment.set(
            mc_bamboo_protocol::packet::play::EquipmentSlot::Head,
            mc_bamboo_protocol::item::ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1),
        );
    });
    world.spawn_entity(rider);
}

/// `SkeletonHorse.setTrap(true)` 挂上优先级 1 的 `SkeletonTrapGoal`；其它目标表为空。
pub(super) fn trap_goals() -> GoalSelector {
    GoalSelector::from_goals(vec![WrappedGoal {
        priority: 1,
        running: false,
        goal: Goal::SkeletonTrap,
    }])
}

/// `SkeletonHorse.setTrap`：挂上 / 摘掉优先级 1 的 `SkeletonTrapGoal`。
pub fn set_trap(mob: &mut Mob, trap: bool) {
    if let Some(state) = mob.skeleton_horse.as_mut()
        && state.trap == trap
    {
        return;
    }
    if let Some(state) = mob.skeleton_horse.as_mut() {
        state.trap = trap;
    }
    mob.goals.goals.retain(|goal| goal.goal != Goal::SkeletonTrap);
    if trap {
        mob.goals.goals.push(WrappedGoal {
            priority: 1,
            running: false,
            goal: Goal::SkeletonTrap,
        });
    }
}
