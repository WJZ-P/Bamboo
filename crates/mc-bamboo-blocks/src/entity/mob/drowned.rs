//! 溺尸：官方 `Zombie` → `Drowned`。属性、近战、破门、增援和溺水转化都复用僵尸那一套；
//! 自己加的是两栖寻路（水里也走）、会游泳的移动控制、只在暗处或目标在水里时才打、
//! 水上 / 水下的叫声、生成时的手持物（三叉戟 / 钓鱼竿）和 3% 的鹦鹉螺壳。
//!
//! 三叉戟投掷（`DrownedTridentAttackGoal` + `ThrownTrident`）本轮没做：投掷物实体本身
//! 还没有，见 README 的已知限制。
//!
//! Drowned: vanilla `Zombie` → `Drowned`. Attributes, melee, door breaking, reinforcements
//! and the drowning conversion all reuse the zombie code. Its own parts are amphibious
//! pathfinding, the swimming move control, attacking only in the dark or against a wet
//! target, the water/land sound variants, the spawn equipment (trident or fishing rod) and
//! the 3% nautilus shell. Trident throwing is not modelled because there is no thrown
//! trident entity yet; see the README limitations.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{items, registries::attribute, tags::fluid_tags};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob, MobType,
    attributes::Attributes,
    control::{self, Operation},
    goals::{CombatTarget, Goal, GoalSelector, TargetType},
    living,
    navigation::Navigation,
    spawn::SpawnReason,
};
use crate::{
    entity::{
        Entity,
        ctx::{BlockView, Command, EntityCtx},
    },
    world::World,
};

/// `Zombie.drowningTracker` 的 600 刻浸水、300 刻转化。
///
/// The 600 afflicted ticks and 300 conversion ticks of `Zombie.drowningTracker`.
const DROWNING_TICKS: i32 = 600;
const DROWNED_CONVERSION_TICKS: i32 = 300;

/// `Zombie.getConversionSound()`：世界事件 1040。
///
/// `Zombie.getConversionSound()`: level event 1040.
const ZOMBIE_CONVERTED_TO_DROWNED: i32 = 1040;

/// `Drowned.DATA_*` 的存档键名由 `ConversionTracker` 的两参数给。
pub(super) const DROWNED_CONVERSION_KEY: &str = "DrownedConversionTime";
pub(super) const IN_WATER_TIME_KEY: &str = "InWaterTime";

/// `BiomeTags.MORE_FREQUENT_DROWNED_SPAWNS`。
const MORE_FREQUENT_DROWNED_SPAWNS: &str = "minecraft:more_frequent_drowned_spawns";

/// `Drowned.createAttributes()`：`Zombie.createAttributes()` 加 `STEP_HEIGHT = 1.0`。
///
/// `Drowned.createAttributes()`: the zombie attributes plus `STEP_HEIGHT = 1.0`.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::zombie();
    table.set_base(attribute::STEP_HEIGHT, 1.0);
    table
}

/// `Zombie.registerGoals` + `Drowned.addBehaviourGoals`，按注册顺序。
/// 没建模的目标占着位置（`Unavailable`），保证标志位锁的顺序和官方一致。
///
/// `Zombie.registerGoals` plus `Drowned.addBehaviourGoals`, in registration order.
/// Goals that are not modelled keep their slot as `Unavailable` so the flag locking order
/// matches vanilla.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    let goals = vec![
        (4, Goal::TurtleEgg { speed: 1.0 }),
        (
            8,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            8,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
        (
            1,
            Goal::DrownedGoToWater {
                speed: 1.0,
                wanted: Vec3::default(),
            },
        ),
        (2, Goal::Unavailable("DrownedTridentAttackGoal")),
        (
            2,
            Goal::MeleeAttack {
                speed: 1.0,
                ticks_until_next_attack: 0,
                ticks_until_path_recalc: 0,
                pathed_target: Vec3::default(),
                raise_arm_ticks: 0,
                last_can_use_check: 0,
                start_path: None,
            },
        ),
        (5, Goal::Unavailable("DrownedGoToBeachGoal")),
        (
            6,
            Goal::DrownedSwimUp {
                speed: 1.0,
                stuck: false,
            },
        ),
        (
            7,
            Goal::RandomStroll {
                speed: 1.0,
                interval: 120,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
    ];
    let _ = navigation;
    GoalSelector::from_goals(goals)
}

/// `Drowned.addBehaviourGoals` 里的 `targetSelector`。玩家的那一条带 `okTarget` 条件，
/// 美西螈占位保住顺序。
///
/// The `targetSelector` of `Drowned.addBehaviourGoals`. The player entry carries the
/// `okTarget` condition; the axolotl entry keeps its slot.
pub(super) fn targets() -> GoalSelector {
    let nearest = |kind, interval| Goal::NearestAttackableTarget {
        kind,
        interval,
        unseen_ticks: 0,
        found: None,
    };
    GoalSelector::from_goals(vec![
        (1, Goal::HurtByTarget { timestamp: -1 }),
        (
            2,
            nearest(TargetType::Player, super::goals::reduced_tick_delay(10)),
        ),
        (
            3,
            nearest(TargetType::Villager, super::goals::reduced_tick_delay(10)),
        ),
        (
            3,
            nearest(TargetType::IronGolem, super::goals::reduced_tick_delay(10)),
        ),
        (
            3,
            nearest(TargetType::Axolotl, super::goals::reduced_tick_delay(10)),
        ),
        (
            5,
            nearest(TargetType::Turtle, super::goals::reduced_tick_delay(10)),
        ),
    ])
}

/// `Drowned.okTarget`：目标非空且（外面不亮 或 目标在水里）。
///
/// `Drowned.okTarget`: a target that exists and is either wet or in the dark.
pub(super) fn ok_target(ctx: &dyn EntityCtx, target: Option<(Vec3, bool)>) -> bool {
    let Some((_position, in_water)) = target else {
        return false;
    };
    !ctx.blocks().is_bright_outside() || in_water
}

/// `Drowned.wantsToSwim()`：在找陆地，或者目标在水里。`target_in_water` 由调用方保证
/// 只在有目标且目标在水里时为真。
///
/// `Drowned.wantsToSwim()`: searching for land, or the target is in water. Callers only
/// pass `true` for `target_in_water` when a target exists and is wet.
pub(super) fn wants_to_swim(mob: &Mob, target_in_water: bool) -> bool {
    mob.searching_for_land || target_in_water
}

/// `Drowned.updateSwimming()`：`isEffectiveAi() && isUnderWater() && wantsToSwim()`。
/// 眼睛是否在水里读的是上一刻的流体跟踪。
///
/// `Drowned.updateSwimming()`: `isEffectiveAi() && isUnderWater() && wantsToSwim()`. The
/// eye check reads last tick's fluid trackers.
pub(super) fn update_swimming(mob: &mut Mob, target_in_water: bool) {
    let under_water = living::eye_in_fluid(mob, fluid_tags::WATER);
    let swimming = !mob.no_ai && under_water && wants_to_swim(mob, target_in_water);
    mob.swimming = swimming;
}

/// `Drowned.travelInWater`：在水下且想游时用 0.01 的相对移动，移动后速度乘 0.9；
/// 否则走普通的 `LivingEntity.travelInWater`。
///
/// `Drowned.travelInWater`: under water and wanting to swim, move at 0.01 relative and
/// scale the velocity by 0.9; otherwise the ordinary `LivingEntity.travelInWater`.
pub(super) fn travel_in_water(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    input: Vec3,
    target_in_water: bool,
) -> bool {
    let under_water = living::eye_in_fluid(mob, fluid_tags::WATER);
    if !under_water || !wants_to_swim(mob, target_in_water) {
        return false;
    }
    living::move_relative(me, 0.01, input);
    let movement = me.delta;
    super::motion::move_living(me, mob, ctx, movement);
    me.delta = me.delta.scale(0.9);
    true
}

/// `DrownedMoveControl.tick`：在水里且想游时直接朝目标加速（不寻路速度），否则退回普通
/// `MoveControl.tick`，空中多一个 `-0.008` 的重力补偿。
///
/// `DrownedMoveControl.tick`: while swimming, accelerate straight at the wanted point;
/// otherwise the ordinary `MoveControl.tick`, with an extra `-0.008` in the air.
pub(super) fn move_control_tick(
    me: &mut Entity,
    mob: &mut Mob,
    view: &dyn BlockView,
    target: Option<CombatTarget>,
    target_in_water: bool,
) {
    let wants_to_swim = wants_to_swim(mob, target_in_water);
    if wants_to_swim && me.in_water {
        if target.is_some_and(|target| target.position.y > me.position.y) || mob.searching_for_land
        {
            me.delta = me.delta.add(Vec3::new(0.0, 0.002, 0.0));
        }
        if mob.mover.operation != Operation::MoveTo || mob.navigation.is_done() {
            living::set_speed(mob, 0.0);
            return;
        }
        let dx = mob.mover.wanted_x - me.position.x;
        let dy = mob.mover.wanted_y - me.position.y;
        let dz = mob.mover.wanted_z - me.position.z;
        let dd = (dx * dx + dy * dy + dz * dz).sqrt();
        let dy = dy / dd;
        let yaw = (mth::atan2(dz, dx) * 57.295_776_367_187_5) as f32 - 90.0;
        me.y_rot = control::move_rotlerp(me.y_rot, yaw, 90.0);
        mob.y_body_rot = me.y_rot;
        let target_speed =
            (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
        let new_speed = mob.speed + 0.125 * (target_speed - mob.speed);
        living::set_speed(mob, new_speed);
        let (dx, dy, dz) = (dx as f32, dy as f32, dz as f32);
        me.delta = me.delta.add(Vec3::new(
            f64::from(new_speed * dx * 0.005),
            f64::from(new_speed * dy * 0.1),
            f64::from(new_speed * dz * 0.005),
        ));
    } else {
        if !me.on_ground {
            me.delta = me.delta.add(Vec3::new(0.0, -0.008, 0.0));
        }
        living::move_control_tick(me, mob, view);
    }
}

/// `Drowned.closeToNextPos()`：路径终点在自己 4 格（平方）以内。
///
/// `Drowned.closeToNextPos()`: the path target is within four squared blocks.
pub(super) fn close_to_next_pos(me: &Entity, mob: &Mob) -> bool {
    let Some(path) = &mob.navigation.path else {
        return false;
    };
    let target = path.target;
    let dx = me.position.x - (f64::from(target.x) + 0.5);
    let dy = me.position.y - f64::from(target.y);
    let dz = me.position.z - (f64::from(target.z) + 0.5);
    dx * dx + dy * dy + dz * dz < 4.0
}

/// `Zombie.tick` 结尾的 `drowningTracker.tick()`：眼睛在水里累计 600 刻后，再过 300 刻
/// 变成溺尸。和平难度下 `EntityType.create` 返回空，什么都不发生。
///
/// The `drowningTracker.tick()` at the end of `Zombie.tick`: 600 submerged ticks, then 300
/// ticks later the zombie becomes a drowned. In peaceful `EntityType.create` returns null.
pub(super) fn tick_drowning(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !living::is_alive(me, mob) || mob.no_ai {
        return;
    }
    let afflicted = living::eye_in_fluid(mob, fluid_tags::WATER);
    let due = mob
        .drown_conversion
        .tick(afflicted, DROWNING_TICKS, DROWNED_CONVERSION_TICKS);
    mob.drowned_converting = mob.drown_conversion.converting;
    if !due || ctx.blocks().difficulty() == 0 {
        return;
    }
    ctx.emit(
        Command::ConvertMob {
            from: Box::new(mob.clone()),
            to: MobType::Drowned,
            level_event: ZOMBIE_CONVERTED_TO_DROWNED,
            zombie_attributes: true,
        },
        me,
    );
    me.discard();
}

/// `Drowned.populateDefaultEquipmentSlots`：`nextFloat() > 0.9` 时，`nextInt(16)` 小于 10
/// 拿三叉戟，否则拿钓鱼竿。它不调用 `super`，所以没有盔甲。
///
/// `Drowned.populateDefaultEquipmentSlots`: on `nextFloat() > 0.9`, a `nextInt(16)` below
/// ten gives a trident and otherwise a fishing rod. It does not call `super`, so no armour.
pub(super) fn populate_equipment(mob: &mut Mob, random: &crate::random::LegacyRandom) {
    if random.next_float() <= 0.9 {
        return;
    }
    let item = if random.next_int(16) < 10 {
        items::TRIDENT
    } else {
        items::FISHING_ROD
    };
    mob.equipment
        .set(EquipmentSlot::MainHand, ItemStack::new(item, 1));
}

/// `Drowned.finalizeSpawn` 在 `Zombie.finalizeSpawn` 之后的部分：3% 的副手鹦鹉螺壳，
/// 以及自然生成时带三叉戟的有 50% 变成鹦鹉螺僵尸的骑手。后者 Bamboo 没有鹦鹉螺僵尸，
/// 只照官方取那一次随机数，不生成坐骑。
///
/// The part of `Drowned.finalizeSpawn` after `Zombie.finalizeSpawn`: the 3% offhand
/// nautilus shell, and the 50% zombie-nautilus jockey for trident-carrying natural spawns.
/// Bamboo has no zombie nautilus, so the draw happens but no mount is created.
pub(super) fn after_zombie_finalize(world: &World<'_>, entity: &mut Entity, reason: SpawnReason) {
    let random = world.random();
    let mut place_shell = false;
    entity.with_mob(|_, mob| {
        if mob.equipment.get(EquipmentSlot::OffHand).is_empty() && random.next_float() < 0.03 {
            mob.equipment.set(
                EquipmentSlot::OffHand,
                ItemStack::new(items::NAUTILUS_SHELL, 1),
            );
            place_shell = true;
        }
    });
    if place_shell {
        entity.with_mob(|_, mob| mob.equipment.set_drop_chance(EquipmentSlot::OffHand, 2.0));
    }
    let natural = reason == SpawnReason::Natural;
    let trident = entity
        .kind
        .mob()
        .is_some_and(|mob| mob.equipment.get(EquipmentSlot::MainHand).item == items::TRIDENT);
    if !(natural && trident) {
        return;
    }
    let roll = random.next_float();
    if roll >= 0.5 {
        return;
    }
    let baby = entity.kind.mob().is_some_and(Mob::is_baby);
    let biome = world.biome_id(entity.block_position());
    if !baby && !super::spawn::biome_tag_contains(MORE_FREQUENT_DROWNED_SPAWNS, biome) {
        // 官方的 `ZombieNautilus.create` 在这里生成坐骑并让溺尸骑上去；Bamboo 没有这个实体。
        //
        // Vanilla creates and mounts a zombie nautilus here; Bamboo has no such entity.
    }
}

/// `SpawnPlacementTypes.IN_WATER.isSpawnPositionOk`：水格里、上面一格不是红石导体。
///
/// `SpawnPlacementTypes.IN_WATER.isSpawnPositionOk`: a water block whose block above is
/// not a redstone conductor.
pub(super) fn placement_ok(world: &World, pos: BlockPos) -> bool {
    if !world.world_border().is_within_bounds_pos(pos) {
        return false;
    }
    let above = BlockPos::new(pos.x, pos.y + 1, pos.z);
    world.fluid_state(pos).is_tagged(fluid_tags::WATER)
        && !world.block_state(above).is_redstone_conductor()
}

/// `Drowned.checkDrownedSpawnRules`：脚下必须是水；浅水里只有 1/40（沼泽类群系 1/15），
/// 并且海平面 5 格以下。
///
/// `Drowned.checkDrownedSpawnRules`: water below, one in forty (one in fifteen in the
/// more-frequent biomes) and at least five blocks below sea level.
pub(super) fn check_spawn_rules(world: &World, pos: BlockPos) -> bool {
    let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
    if !world.fluid_state(below).is_tagged(fluid_tags::WATER) {
        return false;
    }
    let biome = world.biome_id(pos);
    let random = world.random();
    let can_monster_spawn = world.level().difficulty_id() != 0
        && super::natural::is_dark_enough_to_spawn(world, pos)
        && world.fluid_state(pos).is_tagged(fluid_tags::WATER);
    let more_frequent = super::spawn::biome_tag_contains(MORE_FREQUENT_DROWNED_SPAWNS, biome);
    if more_frequent {
        random.next_int(15) == 0 && can_monster_spawn
    } else {
        random.next_int(40) == 0 && pos.y < world.sea_level() - 5 && can_monster_spawn
    }
}

/// `Drowned` 的叫声按 `isInWater()` 分水上、水下两种。
///
/// The drowned sound variants, split by `isInWater()`.
pub(super) const fn ambient_sound(in_water: bool) -> &'static str {
    if in_water {
        "minecraft:entity.drowned.ambient_water"
    } else {
        "minecraft:entity.drowned.ambient"
    }
}

/// `Drowned.getHurtSound`。
pub(super) const fn hurt_sound(in_water: bool) -> &'static str {
    if in_water {
        "minecraft:entity.drowned.hurt_water"
    } else {
        "minecraft:entity.drowned.hurt"
    }
}

/// `Drowned.getDeathSound`。
pub(super) const fn death_sound(in_water: bool) -> &'static str {
    if in_water {
        "minecraft:entity.drowned.death_water"
    } else {
        "minecraft:entity.drowned.death"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 属性是僵尸那一套加 `STEP_HEIGHT = 1.0`。
    ///
    /// The zombie attributes plus a step height of one.
    #[test]
    fn attributes_add_the_step_height() {
        let table = attributes();
        assert_eq!(table.base(attribute::STEP_HEIGHT), 1.0);
        assert_eq!(table.base(attribute::MOVEMENT_SPEED), f64::from(0.23_f32));
        assert_eq!(table.value(attribute::MAX_HEALTH), 20.0);
        assert_eq!(table.value(attribute::ATTACK_DAMAGE), 3.0);
    }

    /// 目标只在暗处或目标在水里时成立。
    ///
    /// A target only counts in the dark or when it is wet.
    #[test]
    fn ok_target_needs_darkness_or_water() {
        let mut entity = Entity::by_mob_type(MobType::Drowned, Vec3::default());
        entity.with_mob(|_, mob| {
            assert!(wants_to_swim(mob, true));
            assert!(!wants_to_swim(mob, false));
            mob.searching_for_land = true;
            assert!(wants_to_swim(mob, false));
        });
    }

    /// 目标列表按官方顺序注册，玩家的间隔和僵尸一样是折半的 10。
    ///
    /// The target list keeps the vanilla order; the player interval is the halved ten.
    #[test]
    fn goals_and_targets_follow_vanilla() {
        let mob = Mob::new(MobType::Drowned);
        let names: Vec<(i32, &str)> = mob
            .goals
            .goals
            .iter()
            .map(|wrapped| (wrapped.priority, wrapped.goal.name()))
            .collect();
        assert_eq!(
            names,
            vec![
                (4, "ZombieAttackTurtleEggGoal"),
                (8, "LookAtPlayerGoal"),
                (8, "RandomLookAroundGoal"),
                (1, "DrownedGoToWaterGoal"),
                (2, "DrownedTridentAttackGoal"),
                (2, "ZombieAttackGoal"),
                (5, "DrownedGoToBeachGoal"),
                (6, "DrownedSwimUpGoal"),
                (7, "RandomStrollGoal"),
            ]
        );
        assert_eq!(mob.target_goals.goals.len(), 6);
    }
}
