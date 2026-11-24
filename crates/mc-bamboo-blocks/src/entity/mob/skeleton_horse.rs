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

use super::goals::{Goal, WrappedGoal};
use super::{Mob, MobType, attributes::Attributes};
use crate::{
    entity::{Entity, ctx::BlockView, ctx::EntityCtx},
    random::RandomSource,
    world::World,
};

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
    attrs.set_base(attribute::MAX_HEALTH, entity_types::ZOMBIE_HORSE_MAX_HEALTH);
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
pub fn trap_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) -> TrapSpawn {
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
        .map(|_| (random.triangle(0.0, 1.1485), random.triangle(0.0, 1.1485)))
        .collect();
    me.random = random;
    TrapSpawn {
        position: me.position,
        pushes,
        effective: ctx.blocks().effective_difficulty(me.block_position()),
        rider_seeds: Vec::new(),
    }
}

/// 一次陷阱触发的落地事实。
#[derive(Debug, Clone, PartialEq)]
pub struct TrapSpawn {
    pub position: Vec3,
    /// 三匹新骷髅马分别被推的 (dx, dz)。
    pub pushes: Vec<(f64, f64)>,
    pub effective: f32,
    /// 对拍夹具用：给四名骑士指定的实体随机源状态；运行时为空。
    pub rider_seeds: Vec<i64>,
}

/// `SkeletonTrapGoal.createSkeleton`：先跑 `Skeleton.finalizeSpawn`（装备生成与
/// `populateDefaultEquipmentEnchantments` 都在世界随机源上），再把主手和头盔的
/// `enchantments` 组件清成空、按 `MOB_SPAWN_EQUIPMENT` 提供器用骷髅自己的
/// 实体随机源重抽一次。头盔若为空就补一顶铁头盔。
///
/// Vanilla `createSkeleton`: `finalizeSpawn`, then clear and re-enchant MAINHAND
/// and HEAD with the `MOB_SPAWN_EQUIPMENT` provider using the skeleton's own
/// random; fill an empty head slot with an iron helmet.
fn trap_rider(world: &mut World<'_>, position: Vec3, special: f32, seed: Option<i64>) -> Entity {
    use mc_bamboo_protocol::packet::play::EquipmentSlot;
    // `EntityType.create` 先分配实体 ID / 种子，再跑 `finalizeSpawn`；官方
    // `createSkeleton` 的 `enchant` 用的是骷髅自己的随机源，所以先 `prepare`。
    let mut rider = world.prepare_entity(Entity::by_mob_type(MobType::Skeleton, position));
    if let Some(seed) = seed {
        rider.random = crate::entity::JavaRandom::from_state(seed);
    }
    super::skeleton::finalize_spawn(world, &mut rider);
    let difficulty = crate::entity::ctx::BlockView::difficulty(world);
    let _ = difficulty;
    rider.with_mob(|me, mob| {
        mob.invulnerable_time = 60;
        mob.persistence_required = true;
        let head = EquipmentSlot::Head;
        if mob.equipment.get(head).is_empty() {
            mob.equipment.set(
                head,
                mc_bamboo_protocol::item::ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1),
            );
        }
        // `enchantItemFromProvider` 用骷髅自己的 `getRandom()`，不是世界随机源。
        let random = crate::entity::EntityRandom(std::cell::RefCell::new(me.random));
        for slot in [EquipmentSlot::MainHand, head] {
            let mut stack = mob.equipment.get(slot).clone();
            // `ItemStack.set(ENCHANTMENTS, EMPTY)`：清掉 `finalizeSpawn` 可能给的附魔。
            let mut patch = stack.patch();
            patch.remove(mc_bamboo_data::registries::data_component_type::ENCHANTMENTS);
            (stack.added, stack.removed) = patch.to_opaque();
            world
                .level()
                .enchant_mob_spawn_equipment_from(&mut stack, special, &random);
            mob.equipment.set(slot, stack);
        }
        me.random = random.0.into_inner();
    });
    rider
}

/// `SkeletonTrapGoal.tick` 的落地：一道视觉闪电、原马上的骷髅和三匹新的骷髅马骑士。
/// 生成顺序照官方 `tick`：闪电 → 第一名骑士 → （马、骑士）× 3。四名骑士都
/// `startRiding` 各自的骷髅马；原马正在 tick、不在实体表里，直接改两边的链接。
///
/// Vanilla `SkeletonTrapGoal.tick`: a visual bolt, the rider on the trap horse and
/// three new horse/rider pairs, each rider starting to ride its horse. The trap
/// horse is mid-tick and not in the store, so its links are set directly.
pub fn spawn_trap(world: &mut World, vehicle: &mut Entity, facts: TrapSpawn) {
    let bolt = crate::entity::lightning::entity(facts.position, true);
    world.spawn_entity(bolt);
    let special = ((facts.effective - 2.0) / 2.0).clamp(0.0, 1.0);
    // 骑在原陷阱马上的第一名骷髅。
    let first_seed = facts.rider_seeds.first().copied();
    let rider = trap_rider(world, facts.position, special, first_seed);
    let first = world.add_prepared_entity(rider);
    world.with_entity_mut(first, &mut |entity| entity.vehicle = Some(vehicle.id));
    vehicle.with_mob(|_, mob| {
        crate::entity::riding::add_passenger(&mut mob.passengers, first, false);
    });
    for (index, (dx, dz)) in facts.pushes.iter().enumerate() {
        let seed = facts.rider_seeds.get(index + 1).copied();
        let mut horse = Entity::by_mob_type(MobType::SkeletonHorse, facts.position);
        horse.with_mob(|_, mob| {
            if let Some(state) = mob.equine.as_mut() {
                state.tame = true;
            }
            mob.age = 0;
            mob.invulnerable_time = 60;
            mob.persistence_required = true;
        });
        horse.delta = Vec3::new(*dx, 0.0, *dz);
        let horse_id = world.spawn_entity(horse);
        let rider = trap_rider(world, facts.position, special, seed);
        let rider_id = world.add_prepared_entity(rider);
        world.start_riding(rider_id, horse_id);
    }
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
    mob.goals
        .goals
        .retain(|goal| goal.goal != Goal::SkeletonTrap);
    if trap {
        mob.goals.goals.push(WrappedGoal {
            priority: 1,
            running: false,
            goal: Goal::SkeletonTrap,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, World, testing::TestLevel};

    /// 陷阱触发后四名骑士各自坐在一匹骷髅马上：原陷阱马一名、三匹新马各一名。
    ///
    /// A trap seats one rider on the trap horse and one on each of the three new
    /// horses.
    #[test]
    fn a_trap_seats_every_rider_on_a_horse() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let position = Vec3::new(0.5, 64.0, 0.5);
        let mut trap = Entity::by_mob_type(MobType::SkeletonHorse, position);
        // 正在 tick 的原马不在实体表里；表里那份只是占一个 uid。
        let uid = world.spawn_entity(trap.clone());
        trap.id = uid;
        spawn_trap(
            &mut world,
            &mut trap,
            TrapSpawn {
                position,
                pushes: vec![(0.0, 0.0); 3],
                effective: 0.0,
                rider_seeds: Vec::new(),
            },
        );
        let passengers = trap.kind.mob().expect("a horse").passengers.clone();
        assert_eq!(passengers.len(), 1);
        let stored = world.live_entities();
        let first = stored
            .iter()
            .find(|entity| entity.id == passengers[0])
            .expect("the first rider is in the table");
        assert_eq!(first.vehicle, Some(uid));

        let horses: Vec<&Entity> = stored
            .iter()
            .filter(|entity| {
                entity
                    .kind
                    .mob()
                    .is_some_and(|mob| mob.mob_type == MobType::SkeletonHorse)
            })
            .collect();
        assert_eq!(horses.len(), 4, "the trap horse and three new ones");
        let mut riders = 0;
        for horse in horses {
            for &rider in &horse.kind.mob().expect("a horse").passengers {
                let entity = stored
                    .iter()
                    .find(|entity| entity.id == rider)
                    .expect("the rider is in the table");
                assert_eq!(entity.vehicle, Some(horse.id));
                riders += 1;
            }
        }
        // 表里的原马副本没有乘客（真正的原马正被 tick），三匹新马各一名。
        assert_eq!(riders, 3);
    }
}
