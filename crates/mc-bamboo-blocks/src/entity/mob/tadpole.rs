//! 蝌蚪：官方 `Tadpole` 和 `TadpoleAi`。它是 `AbstractFish` 的子类，鱼的三个目标（惊慌、
//! 躲玩家、随机游泳）照样注册，另外再跑一个大脑（核心和闲逛两个活动）；移动控制和青蛙
//! 一样是 `SmoothSwimmingMoveControl`，看的控制是 `SmoothSwimmingLookControl`。年龄到
//! 24000 刻变成青蛙。
//!
//! Tadpoles: vanilla `Tadpole` and `TadpoleAi`. As an `AbstractFish` subclass it keeps the
//! three fish goals (panic, avoid players, random swimming) and additionally runs a brain
//! with core and idle activities. Its move control is the frog's
//! `SmoothSwimmingMoveControl` and its look control `SmoothSwimmingLookControl`. At an age
//! of 24000 ticks it turns into a frog.

use mc_bamboo_core::{Vec3, mth};
use mc_bamboo_data::{items, registries::attribute};

use super::{
    Mob, MobEvent, MobType,
    attributes::Attributes,
    brain::{
        Activity, ActivityData, Brain,
        behavior::{Gate, Kind, MoveToTargetSink, Speed, Trigger},
        memory::{MemoryStatus::ValueAbsent, MemoryType},
        sensor::SensorKind,
    },
    control::rotate_towards,
    frog,
    interact::{MobInteractRequest, MobInteraction},
    living, spawn,
};
use crate::{
    InteractionResult,
    entity::{
        Entity,
        ctx::{Command, EntityCtx},
    },
    world::World,
};

/// `EntityTypes.TADPOLE`：0.4×0.3，眼高 `0.19500001F`。
///
/// `EntityTypes.TADPOLE`: 0.4 by 0.3 with a `0.19500001F` eye.
pub const fn dimensions() -> (f32, f32, f32) {
    (0.4, 0.3, 0.195_000_01)
}

/// `Tadpole.ticksToBeFrog`。
///
/// `Tadpole.ticksToBeFrog`.
pub const TICKS_TO_BE_FROG: i32 = 24000;

/// `Tadpole.createAttributes`：动物那一套，速度 1.0、生命 6。
///
/// `Tadpole.createAttributes`: the animal set with 1.0 speed and 6 health.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    for (id, base) in [
        (attribute::MOVEMENT_SPEED, 1.0),
        (attribute::MAX_HEALTH, 6.0),
    ] {
        // 改的是属性表的缺省值，不创建实例（实例会被存进存档）。
        // This changes the supplier default without creating an instance (instances are saved).
        table.set_base(id, base);
    }
    table
}

/// `Tadpole.BRAIN_PROVIDER`：四个传感器和 `TadpoleAi.getActivities()`。
///
/// `Tadpole.BRAIN_PROVIDER`: four sensors and `TadpoleAi.getActivities()`.
pub(super) fn brain() -> Brain {
    Brain::new(
        &[
            SensorKind::NearestLivingEntities,
            SensorKind::NearestPlayers,
            SensorKind::HurtBy,
            SensorKind::FrogTemptations,
        ],
        vec![core_activity(), idle_activity()],
    )
}

/// `TadpoleAi.initCoreActivity`。
fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::AnimalPanic {
                speed: 2.0,
                environmental: false,
                armadillo: false,
                camel: false,
            },
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            Kind::CountDownCooldown(MemoryType::TemptationCooldownTicks),
        ],
    )
}

/// `TadpoleAi.initIdleActivity`。
fn idle_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Idle,
        vec![
            (
                0,
                Kind::LookAtPlayerSometimes {
                    max_dist: 6.0,
                    min_interval: 30,
                    max_interval: 60,
                    ticks_until_next_start: 0,
                },
            ),
            (
                1,
                Kind::FollowTemptation {
                    speed: Speed::fixed(1.25),
                    close_adult: 2.5,
                    close_baby: 2.5,
                },
            ),
            (
                2,
                Kind::Gate(Box::new(Gate::ordered_try_all(
                    vec![(MemoryType::WalkTarget, ValueAbsent)],
                    vec![
                        (Kind::Frog(frog::Behavior::SwimStroll { speed: 0.5 }), 2),
                        (
                            Kind::WalkTargetFromLook {
                                speed: Speed::fixed(0.5),
                                close_enough: 3,
                            },
                            3,
                        ),
                        (Kind::TriggerIf(Trigger::InWater), 5),
                    ],
                ))),
            ),
        ],
    )
}

/// `Tadpole.customServerAiStep` 里 `super` 之前的部分：大脑和 `TadpoleAi.updateActivity`。
///
/// The part of `Tadpole.customServerAiStep` before `super`: the brain and
/// `TadpoleAi.updateActivity`.
pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if let Some(mut brain) = mob.brain.take() {
        brain.tick(me, mob, ctx);
        brain.set_active_activity_to_first_valid(&[Activity::Idle]);
        mob.brain = Some(brain);
    }
}

/// `SmoothSwimmingLookControl(mob, 10).tick`：有看的目标时头朝目标再偏 20 度、俯仰偏
/// 10 度；没有时路走完了才把俯仰慢慢归零，头转回身体的方向。头偏出身体 10 度以上时身体
/// 每刻跟 4 度。
///
/// `SmoothSwimmingLookControl(mob, 10).tick`: with a look target the head turns to it plus
/// 20 degrees and the pitch plus 10; without one the pitch eases back to zero once the
/// path is done and the head returns to the body. A head more than 10 degrees off pulls
/// the body along by 4 degrees a tick.
pub(super) fn look_control_tick(me: &Entity, mob: &mut Mob) {
    const MAX_Y_ROT_FROM_CENTER: f32 = 10.0;
    if mob.look.look_at_cooldown > 0 {
        mob.look.look_at_cooldown -= 1;
        if let Some(target) = super::control::look_y_rot(&mob.look, me.position.x, me.position.z) {
            mob.y_head_rot =
                rotate_towards(mob.y_head_rot, target + 20.0, mob.look.y_max_rot_speed);
        }
        let eye_y = me.position.y + f64::from(mob.eye_height);
        if let Some(target) =
            super::control::look_x_rot(&mob.look, me.position.x, eye_y, me.position.z)
        {
            mob.x_rot = rotate_towards(mob.x_rot, target + 10.0, mob.look.x_max_rot_angle);
        }
    } else {
        if mob.navigation.is_done() {
            mob.x_rot = rotate_towards(mob.x_rot, 0.0, 5.0);
        }
        mob.y_head_rot = rotate_towards(mob.y_head_rot, mob.y_body_rot, mob.look.y_max_rot_speed);
    }
    let head_diff_body = mth::wrap_degrees(mob.y_head_rot - mob.y_body_rot);
    if head_diff_body < -MAX_Y_ROT_FROM_CENTER {
        mob.y_body_rot -= 4.0;
    } else if head_diff_body > MAX_Y_ROT_FROM_CENTER {
        mob.y_body_rot += 4.0;
    }
}

/// `Tadpole.aiStep` 里 `super.aiStep()` 之后的部分：没锁年龄就长一刻。
///
/// The part of `Tadpole.aiStep` after `super.aiStep()`: one more tick of age unless locked.
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !mob.age_locked {
        set_age(me, mob, ctx, mob.tadpole_age + 1);
    }
}

/// `Tadpole.setAge`：到了 `ticksToBeFrog` 就 `ageUp()` 变成青蛙。
///
/// `Tadpole.setAge`: `ageUp()` into a frog once `ticksToBeFrog` is reached.
pub(super) fn set_age(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, age: i32) {
    mob.tadpole_age = age;
    if age < TICKS_TO_BE_FROG || me.removed {
        return;
    }
    // `afterConversion` 最后的 `this.playSound(TADPOLE_GROW_UP, 0.15F, 1.0F)`。
    // The closing `this.playSound(TADPOLE_GROW_UP, 0.15F, 1.0F)` of `afterConversion`.
    living::play_sound(me, mob, "minecraft:entity.tadpole.grow_up", 0.15, 1.0);
    ctx.emit(
        Command::TadpoleGrowUp {
            from: Box::new(mob.clone()),
        },
        me,
    );
    me.discard();
}

/// 落地 [`Command::TadpoleGrowUp`]：造一只青蛙，按 `ConversionType.SINGLE` 搬状态（不带
/// 装备、不保留捡东西的开关），再跑 `afterConversion`：`finalizeSpawn(CONVERSION)`、
/// `setPersistenceRequired`、`fudgePositionAfterSizeChange`。
///
/// Lands [`Command::TadpoleGrowUp`]: create a frog, move the state as
/// `ConversionType.SINGLE` does (no equipment, pick-up flag not preserved), then run
/// `afterConversion`: `finalizeSpawn(CONVERSION)`, `setPersistenceRequired` and
/// `fudgePositionAfterSizeChange`.
pub(crate) fn apply_grow_up(me: &Entity, from: &Mob, world: &mut World) {
    let mut frog = world.prepare_entity(Entity::by_mob_type(MobType::Frog, Vec3::default()));
    spawn::snap_to(&mut frog, me.position, me.y_rot, from.x_rot);
    frog.delta = me.delta;
    frog.fall_distance = me.fall_distance;
    frog.on_ground = me.on_ground;
    frog.with_mob(|_, mob| {
        mob.fall_flying = from.fall_flying;
        mob.last_hurt_by_player_memory_time = from.last_hurt_by_player_memory_time;
        mob.hurt_time = from.hurt_time;
        mob.y_body_rot = from.y_body_rot;
        super::conversion::copy_common(from, mob, false);
    });
    let biome = world.biome_id(frog.block_position());
    spawn::finalize_cow(MobType::Frog, &mut frog, biome, world.random(), &mut None);
    frog.with_mob(|_, mob| mob.persistence_required = true);
    if let Some(position) = crate::entity::resize::fudge_position(world, &frog, me.dimensions) {
        frog.position = position;
    }
    world.add_prepared_entity(frog);
}

/// `Tadpole.mobInteract` 里桶之前的两支：喂粘液球催熟，金蒲公英锁住 / 解开年龄。
///
/// The two branches of `Tadpole.mobInteract` before the bucket: a slime ball speeds up
/// growth and a golden dandelion locks or unlocks the age.
pub(super) fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> Option<MobInteraction> {
    let item = req.held.item;
    if super::interact::is_food(MobType::Tadpole, item) && !mob.age_locked {
        // `feed`：`ageUp`，再算一个服务端不显示的粒子的位置（三次 `nextDouble`）。
        // `feed`: `ageUp`, then the position of a particle the server never shows (three
        // `nextDouble` draws).
        mob.tadpole_age += feed_age_up(mob.tadpole_age);
        for _ in 0..3 {
            me.random.next_double();
        }
        return Some(MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            ..MobInteraction::pass()
        });
    }
    if item == items::GOLDEN_DANDELION
        && mob.age_lock_particle_timer == 0
        && !MobType::Tadpole.in_entity_tag("minecraft:cannot_be_age_locked")
    {
        // `AgeableMob.setAgeLocked` 加 `Tadpole.setAgeLockedData`。
        mob.age_locked = !mob.age_locked;
        mob.tadpole_age = 0;
        mob.age_lock_particle_timer = 40;
        if mob.age_locked {
            mob.persistence_required = true;
        }
        let here = me.block_position();
        mob.events.push(MobEvent::PlayerSound {
            sound: if mob.age_locked {
                "minecraft:item.golden_dandelion.use"
            } else {
                "minecraft:item.golden_dandelion.unuse"
            },
            position: Vec3::new(
                f64::from(here.x) + 0.5,
                f64::from(here.y) + 0.5,
                f64::from(here.z) + 0.5,
            ),
        });
        return Some(MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            ..MobInteraction::pass()
        });
    }
    None
}

/// `AgeableMob.getSpeedUpSecondsWhenFeeding(getTicksLeftUntilAdult())` 换成刻数：
/// `(int)(ticks / 20 * 0.1F) * 20`。
///
/// `AgeableMob.getSpeedUpSecondsWhenFeeding(getTicksLeftUntilAdult())` in ticks:
/// `(int)(ticks / 20 * 0.1F) * 20`.
pub(super) fn feed_age_up(age: i32) -> i32 {
    let left = (TICKS_TO_BE_FROG - age).max(0);
    ((left / 20) as f32 * 0.1) as i32 * 20
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tadpole_brain_has_vanilla_sensors_and_activities() {
        let brain = brain();
        let kinds: Vec<SensorKind> = brain.sensors.iter().map(|sensor| sensor.kind).collect();
        assert_eq!(
            kinds,
            [
                SensorKind::NearestLivingEntities,
                SensorKind::NearestPlayers,
                SensorKind::HurtBy,
                SensorKind::FrogTemptations,
            ]
        );
        assert_eq!(brain.active_names(), vec!["core", "idle"]);
    }

    #[test]
    fn feeding_speeds_growth_by_a_tenth_of_what_is_left() {
        assert_eq!(feed_age_up(0), 2400);
        assert_eq!(feed_age_up(23990), 0);
        assert_eq!(feed_age_up(30000), 0);
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 6.0);
        assert_eq!(table.value(attribute::MOVEMENT_SPEED), 1.0);
    }
}
