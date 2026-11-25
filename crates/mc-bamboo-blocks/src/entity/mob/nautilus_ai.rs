//! 鹦鹉螺 / 僵尸鹦鹉螺的大脑：官方 `NautilusAi` / `ZombieNautilusAi` 的传感器表、
//! 活动与行为登记表，以及 `customServerAiStep` 里的活动切换。
//!
//! 战斗活动的 `ChargeAttack`  Bamboo 还没有对应的冲撞行为，结构表里按官方类名占位，
//! 运行时的选目标 / 冲刺逻辑用一次性行为近似；见 README 的已知限制。
//!
//! The brains of vanilla `NautilusAi` / `ZombieNautilusAi`: sensor tables, activity
//! and behaviour registration tables, and the activity switch in
//! `customServerAiStep`. Bamboo has no `ChargeAttack` behaviour yet, so that slot is
//! a placeholder in the structure table and the runtime target selection uses
//! one-shot behaviours; see the README's known limits.

use mc_bamboo_core::Vec3;

use super::{
    Mob, MobType,
    brain::{
        Activity, ActivityData, Brain, BrainTemplate, Cx, TemplateCache,
        behavior::{Class, Gate, Kind, MoveToTargetSink, Speed},
        memory::{MemoryStatus, MemoryType, MemoryValue, WalkTarget},
        sensor::{self, SensorKind},
        template_cache,
    },
    nautilus,
};
use crate::entity::{Entity, ctx::EntityCtx};

use MemoryStatus::{ValueAbsent, ValuePresent};
use MemoryType::{
    AttackTarget, BreedTarget, ChargeCooldownTicks, TemptationCooldownTicks, TemptingPlayer,
    WalkTarget as WalkTargetMemory,
};

/// `NautilusAi` / `ZombieNautilusAi` 共用的传感器顺序。
///
/// The shared sensor order of `NautilusAi` and `ZombieNautilusAi`.
const SENSORS: &[SensorKind] = &[
    SensorKind::NearestLivingEntities,
    SensorKind::NearestAdult,
    SensorKind::NearestPlayers,
    SensorKind::HurtBy,
    SensorKind::NautilusTemptations,
];

/// 诱惑速度：鹦鹉螺 1.3，僵尸鹦鹉螺 0.9；停步距离成年 3.5、幼年 2.5。
///
/// Temptation speed: 1.3 for nautiluses, 0.9 for zombie nautiluses; the stop
/// distance is 3.5 for adults and 2.5 for babies.
fn temptation_speed(kind: MobType) -> Speed {
    Speed::Fixed(if kind == MobType::ZombieNautilus {
        0.9
    } else {
        1.3
    })
}

/// `Brain.provider(memories, sensors, activities)`：两种各建一次模板。
///
/// `Brain.provider(memories, sensors, activities)`: one cached template per kind.
pub(in crate::entity::mob) fn brain(kind: MobType) -> Brain {
    static NAUTILUS: TemplateCache = template_cache();
    static ZOMBIE: TemplateCache = template_cache();
    let cache = if kind == MobType::ZombieNautilus {
        &ZOMBIE
    } else {
        &NAUTILUS
    };
    Brain::cached(cache, || {
        BrainTemplate::new(
            SENSORS,
            vec![core_activity(kind), idle_activity(kind), fight_activity()],
        )
    })
}

/// `initCoreActivity`：鹦鹉螺多一个 `AnimalPanic(1.6)`，僵尸版没有。
///
/// `initCoreActivity`: the nautilus adds `AnimalPanic(1.6)`; the zombie version
/// does not.
fn core_activity(kind: MobType) -> ActivityData {
    let mut behaviors = Vec::new();
    if kind == MobType::Nautilus {
        behaviors.push(Kind::AnimalPanic {
            speed: 1.6,
            environmental: false,
            armadillo: false,
            camel: false,
        });
    }
    behaviors.extend([
        Kind::LookAtTargetSink { min: 45, max: 90 },
        Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
        Kind::CountDownCooldown(TemptationCooldownTicks),
        Kind::CountDownCooldown(ChargeCooldownTicks),
        Kind::CountDownCooldown(MemoryType::AttackTargetCooldown),
    ]);
    ActivityData::sequential(Activity::Core, 0, behaviors)
}

/// `initIdleActivity`：鹦鹉螺多一项 `AnimalMakeLove(NAUTILUS, 0.4, 2)`，优先级从 1 起。
///
/// `initIdleActivity`: the nautilus adds `AnimalMakeLove(NAUTILUS, 0.4, 2)`, with
/// priorities starting at 1.
fn idle_activity(kind: MobType) -> ActivityData {
    let mut rows: Vec<(i32, Kind)> = Vec::new();
    let mut priority = 1;
    if kind == MobType::Nautilus {
        rows.push((
            priority,
            Kind::MakeLove {
                partner: MobType::Nautilus,
                speed: 0.4,
                close_enough: 2,
                spawn_child_at: 0,
            },
        ));
        priority += 1;
    }
    rows.push((
        priority,
        Kind::FollowTemptation {
            speed: temptation_speed(kind),
            close_adult: 3.5,
            close_baby: 2.5,
        },
    ));
    priority += 1;
    rows.push((priority, Kind::Nautilus(Behavior::StartAttacking)));
    priority += 1;
    rows.push((
        priority,
        Kind::Gate(Box::new(Gate::ordered_try_all(
            vec![(WalkTargetMemory, ValueAbsent)],
            vec![
                (Kind::Nautilus(Behavior::SwimStroll { speed: 1.0 }), 2),
                (
                    Kind::Nautilus(Behavior::WalkTargetFromLook {
                        speed: Speed::Fixed(1.0),
                        close_enough: 3,
                    }),
                    3,
                ),
            ],
        ))),
    ));
    ActivityData::with_priorities(Activity::Idle, rows)
}

/// `initFightActivity`：`ChargeAttack` + 四条进入条件。
///
/// `initFightActivity`: `ChargeAttack` plus four entry conditions.
fn fight_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Fight,
        vec![(0, Kind::Nautilus(Behavior::ChargeAttack))],
    )
    .when(vec![
        (AttackTarget, ValuePresent),
        (TemptingPlayer, ValueAbsent),
        (BreedTarget, ValueAbsent),
        (ChargeCooldownTicks, ValueAbsent),
    ])
}

/// `Node` 的官方类名，对拍用。`ChargeAttack` 是官方类名；其余一次性行为按 `OneShot`。
///
/// The official class names for oracle dumps. `ChargeAttack` keeps its own name;
/// other one-shots are `OneShot`.
fn behavior_name(behavior: &Behavior) -> &'static str {
    match behavior {
        Behavior::ChargeAttack => "ChargeAttack",
        _ => "OneShot",
    }
}

/// `Behavior` 的运行时类别：冲撞是 `Behavior(80,80)`，其余是一次性触发。
///
/// Runtime class of each behaviour: `ChargeAttack` is `Behavior(80, 80)`, the rest
/// are one-shots.
fn behavior_class(behavior: &Behavior) -> Class {
    match behavior {
        Behavior::ChargeAttack => Class::Timed { min: 80, max: 80 },
        _ => Class::OneShot,
    }
}

/// 鹦鹉螺自己的一次性行为。
///
/// Nautilus-specific one-shot behaviours.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `ChargeAttack`（结构表占位，服务端不模拟冲刺伤害）。
    ///
    /// `ChargeAttack` (a structure-table placeholder; no charge damage server-side).
    ChargeAttack,
    /// `StartAttacking.create(TargetFinder)`：挑一个水里的敌对目标。
    ///
    /// `StartAttacking.create(TargetFinder)`: pick a hostile target in water.
    StartAttacking,
    /// `RandomStroll.swim(1)`：水里的随机游泳漫步。
    ///
    /// `RandomStroll.swim(1)`: a random swim stroll in water.
    SwimStroll { speed: f32 },
    /// `SetWalkTargetFromLookTarget.create(1, 3)`。
    WalkTargetFromLook { speed: Speed, close_enough: i32 },
}

impl Behavior {
    pub(super) fn name(&self) -> &'static str {
        behavior_name(self)
    }

    pub(super) fn class(&self) -> Class {
        behavior_class(self)
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::ChargeAttack => &[
                (AttackTarget, ValuePresent),
                (TemptingPlayer, ValueAbsent),
                (BreedTarget, ValueAbsent),
                (ChargeCooldownTicks, ValueAbsent),
            ],
            Self::SwimStroll { .. } => &[(WalkTargetMemory, ValueAbsent)],
            Self::WalkTargetFromLook { .. } => &[],
            Self::StartAttacking => &[],
        }
    }

    pub(super) fn extra_start(&mut self, _cx: &mut Cx) -> bool {
        true
    }

    pub(super) fn start(&mut self, _cx: &mut Cx) {}

    pub(super) fn can_still_use(&mut self, _cx: &mut Cx) -> bool {
        true
    }

    pub(super) fn tick(&mut self, _cx: &mut Cx) {}

    pub(super) fn stop(&mut self, _cx: &mut Cx) {}

    pub(super) fn trigger(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::ChargeAttack => false,
            Self::StartAttacking => start_attacking(cx),
            Self::SwimStroll { speed } => swim_stroll(cx, *speed),
            Self::WalkTargetFromLook {
                speed,
                close_enough,
            } => walk_target_from_look(cx, *speed, *close_enough),
        }
    }
}

/// `RandomStroll.swim(speed)`：照官方先在水里找一个能游的点，没有就不设走路目标。
///
/// `RandomStroll.swim(speed)`: find a swimmable spot in water; leave the walk
/// target alone when there is none.
fn swim_stroll(cx: &mut Cx, speed: f32) -> bool {
    if !cx.me.in_water {
        return false;
    }
    let pos = super::fish::random_swimmable_pos(cx.me, cx.mob, cx.ctx.blocks(), 10, 7);
    cx.memories.set_or_erase(
        WalkTargetMemory,
        pos.map(|pos| MemoryValue::Walk(WalkTarget::to_point(pos, speed, 0))),
    );
    true
}

/// `SetWalkTargetFromLookTarget`：照着看向的那个位置设走路目标。
///
/// `SetWalkTargetFromLookTarget`: set the walk target to the looked-at position.
fn walk_target_from_look(cx: &mut Cx, speed: Speed, close_enough: i32) -> bool {
    let Some(target) = cx.memories.look_target() else {
        return false;
    };
    cx.memories.set(
        WalkTargetMemory,
        MemoryValue::Walk(WalkTarget {
            target,
            speed: speed.value(cx.me.in_water),
            close_enough,
        }),
    );
    true
}

/// `StartAttacking`：从可见生物里挑一个水里的 `#nautilus_hostiles`（河豚）。
///
/// `StartAttacking`: pick a visible `#nautilus_hostiles` (pufferfish) in water.
fn start_attacking(cx: &mut Cx) -> bool {
    if cx.mob.tame.is_tame() || cx.mob.is_baby() {
        return false;
    }
    let position = cx.me.position;
    let Some(uid) = sensor::find_closest_visible(cx, &mut |view| {
        view.position.distance_to_sqr(position) <= 144.0
            && view.mob_type == Some(MobType::Pufferfish)
    }) else {
        return false;
    };
    cx.memories.set(AttackTarget, MemoryValue::Entity(uid));
    true
}

/// `customServerAiStep`：先跑大脑，再 `updateActivity`，最后 `checkRestriction`。
///
/// `customServerAiStep`: tick the brain, update the activity, then check the home
/// restriction.
pub(in crate::entity::mob) fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
) {
    if let Some(mut brain) = mob.brain.take() {
        let gap = ctx.throttle_plan().decision_gap;
        brain.tick_by(me, mob, ctx, gap);
        brain.set_active_activity_to_first_valid(&[Activity::Fight, Activity::Idle]);
        mob.brain = Some(brain);
    }
    nautilus::check_restriction(me, mob);
}

/// `NautilusAi.initMemories`：从世界随机源抽一次 2400..3600 的攻击冷却。
///
/// `NautilusAi.initMemories`: draw the 2400..3600 attack cooldown from the level
/// random source.
pub fn init_memories(mob: &mut Mob, random: &dyn crate::random::RandomSource) {
    let cooldown = 2400 + random.next_int(1201);
    if let Some(brain) = mob.brain.as_mut() {
        brain
            .memories
            .set(MemoryType::AttackTargetCooldown, MemoryValue::Int(cooldown));
    }
}

/// `Nautilus.getBreedOffspring`：幼崽继承主人的驯服状态。
///
/// `Nautilus.getBreedOffspring`: a baby inherits its parent's owner and tame flag.
pub fn inherit_tame(parent: &Mob, child: &mut Mob) {
    if parent.tame.is_tame() {
        child.tame.owner = parent.tame.owner;
        crate::entity::mob::tame::set_tame(child, true, true);
    }
}

/// `ZombieNautilus.finalizeSpawn`：按群系标签选 `warm` / `temperate`。
///
/// `ZombieNautilus.finalizeSpawn`: choose `warm` / `temperate` from the biome tag.
pub fn finalize_variant(mob: &mut Mob, biome: i32) {
    let warm = super::spawn::biome_tag_contains(nautilus::CORAL_VARIANT_BIOME_TAG, biome);
    mob.nautilus.variant = if warm {
        nautilus::Variant::Warm
    } else {
        nautilus::Variant::Temperate
    }
    .registry_id();
}

/// 玩家骑乘时的朝向：`getRiddenRotation` 把俯仰减半、水平朝向直接跟着玩家。
///
/// The ridden rotation: `getRiddenRotation` halves the pitch and follows the
/// player's yaw directly.
pub fn ridden_rotation(me: &mut Entity, mob: &mut Mob, yaw: f32, pitch: f32) {
    let wrapped = mc_bamboo_core::mth::wrap_degrees(yaw - me.y_rot);
    let y_rot = me.y_rot + wrapped * 0.5;
    mob.x_rot = pitch * 0.5;
    me.y_rot = y_rot;
    mob.y_head_rot = y_rot;
    mob.y_body_rot = y_rot;
    mob.y_rot_o = y_rot;
}

/// `getRiddenInput`：前后输入变成竖直速度、左右输入是横向速度。
///
/// `getRiddenInput`: forward input becomes vertical speed, strafing the horizontal.
pub fn ridden_input(xxa: f32, zza: f32, pitch: f32) -> Vec3 {
    let radians = pitch * 0.017_453_292;
    let mut forward = mc_bamboo_core::mth::cos(f64::from(radians)) as f32;
    let mut vertical = -mc_bamboo_core::mth::sin(f64::from(radians)) as f32;
    if zza < 0.0 {
        forward *= -0.5;
        vertical *= -0.5;
    }
    Vec3::new(f64::from(xxa), f64::from(vertical), f64::from(forward))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两种大脑的结构表：传感器、核心活动、闲逛优先级与战斗进入条件。
    ///
    /// Structure tables of both brains: sensors, core activity, idle priorities and
    /// fight entry conditions.
    #[test]
    fn brain_tables_match_the_vanilla_registration() {
        for kind in [MobType::Nautilus, MobType::ZombieNautilus] {
            let brain = brain(kind);
            let sensors = brain.sensor_table();
            assert_eq!(sensors.len(), 5);
            assert_eq!(sensors[0].0, "NearestLivingEntitySensor");
            assert_eq!(sensors[4].0, "TemptingSensor");
            let table = brain.behavior_table();
            assert!(table.iter().any(|(_, _, name)| name == "ChargeAttack"));
            assert!(
                table.iter().any(|(_, _, name)| name == "AnimalMakeLove")
                    == (kind == MobType::Nautilus)
            );
            let requirements = brain.requirement_table();
            assert!(
                requirements
                    .iter()
                    .any(|(activity, rows)| *activity == "fight" && rows.len() == 4)
            );
            assert_eq!(brain.active_names(), vec!["core", "idle"]);
        }
    }

    /// 只有未驯服、成年的鹦鹉螺才找河豚；变种按群系标签选。
    ///
    /// Only untamed adult nautiluses look for pufferfish; the variant follows the
    /// biome tag.
    #[test]
    fn variant_and_tame_inheritance() {
        let mut parent = Mob::new(MobType::Nautilus);
        crate::entity::mob::tame::set_tame(&mut parent, true, false);
        parent.tame.owner = Some([7; 16]);
        let mut child = Mob::new(MobType::Nautilus);
        inherit_tame(&parent, &mut child);
        assert!(child.tame.is_tame());
        assert_eq!(child.tame.owner, Some([7; 16]));
        let mut zombie = Mob::new(MobType::ZombieNautilus);
        finalize_variant(&mut zombie, mc_bamboo_data::registries::biome::WARM_OCEAN);
        assert_eq!(
            zombie.nautilus.variant,
            nautilus::Variant::Warm.registry_id()
        );
    }
}
